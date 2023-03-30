use criterion::{black_box, criterion_group, criterion_main, Criterion};
use halo2_base::halo2_proofs;
use halo2_base::halo2_proofs::circuit::SimpleFloorPlanner;
use halo2_base::halo2_proofs::halo2curves::bn256::{Bn256, Fr, G1Affine};
use halo2_base::halo2_proofs::plonk::{
    create_proof, keygen_pk, keygen_vk, Circuit, ConstraintSystem,
};
use halo2_base::halo2_proofs::poly::kzg::commitment::KZGCommitmentScheme;
use halo2_base::halo2_proofs::poly::kzg::multiopen::ProverGWC;
use halo2_base::halo2_proofs::poly::{commitment::Params, kzg::commitment::ParamsKZG};
use halo2_base::halo2_proofs::transcript::{
    Blake2bWrite, Challenge255, TranscriptReadBuffer, TranscriptWriterBuffer,
};
use halo2_base::halo2_proofs::{circuit::Layouter, plonk::Error, SerdeFormat};
use halo2_base::{gates::range::RangeConfig, utils::PrimeField, Context};
use halo2_dynamic_sha256::Field;
use halo2_regex::{RegexDef, SubstrDef};
use halo2_rsa::{RSAPubE, RSAPublicKey, RSASignature};
use rand::rngs::OsRng;
use snark_verifier_sdk::CircuitExt;
use std::{
    fs::File,
    io::{prelude::*, BufReader, BufWriter},
    path::Path,
};

use base64::prelude::{Engine as _, BASE64_STANDARD};
use cfdkim::*;
use halo2_base::halo2_proofs::{
    circuit::{floor_planner::V1, Cell, Value},
    dev::{CircuitCost, FailureLocation, MockProver, VerifyFailure},
    plonk::{Any, Column, Instance, ProvingKey, VerifyingKey},
};
use halo2_base::{gates::range::RangeStrategy::Vertical, SKIP_FIRST_PASS};
use halo2_zk_email::EmailVerifyConfig;
use halo2_zk_email::{impl_aggregation_email_verify, impl_email_verify_circuit};
use mailparse::parse_mail;
use num_bigint::BigUint;
use rand::thread_rng;
use rand::Rng;
use rsa::{PublicKeyParts, RsaPrivateKey};
use sha2::{self, Digest, Sha256};

impl_email_verify_circuit!(
    Bench1EmailVerifyConfig,
    Bench1EmailVerifyCircuit,
    1,
    1024,
    "./test_data/regex_header_test1.txt",
    "./test_data/substr_header_bench1_1.txt",
    vec!["./test_data/substr_header_bench1_2.txt"],
    1024,
    "./test_data/regex_body_test1.txt",
    vec!["./test_data/substr_body_bench1_1.txt"],
    2048,
    280,
    9,
    13
);

fn gen_or_get_params(k: usize) -> ParamsKZG<Bn256> {
    let path = format!("params_{}.bin", k);
    match File::open(&path) {
        Ok(f) => {
            let mut reader = BufReader::new(f);
            ParamsKZG::read(&mut reader).unwrap()
        }
        Err(_) => {
            let params = ParamsKZG::<Bn256>::setup(k as u32, OsRng);
            params
                .write(&mut BufWriter::new(File::create(&path).unwrap()))
                .unwrap();
            params
        }
    }
}

fn bench_email_verify1(c: &mut Criterion) {
    let mut group = c.benchmark_group("email bench1 with recursion");
    group.sample_size(10);
    let params = gen_or_get_params(13);
    println!("gen_params");
    let mut rng = thread_rng();
    let _private_key = RsaPrivateKey::new(&mut rng, Bench1EmailVerifyCircuit::<Fr>::BITS_LEN)
        .expect("failed to generate a key");
    let public_key = rsa::RsaPublicKey::from(&_private_key);
    let private_key = cfdkim::DkimPrivateKey::Rsa(_private_key);
    let message = concat!(
        "From: alice@zkemail.com\r\n",
        "\r\n",
        "email was meant for @zkemailverify.",
    )
    .as_bytes();
    let email = parse_mail(message).unwrap();
    let logger = slog::Logger::root(slog::Discard, slog::o!());
    let signer = SignerBuilder::new()
        .with_signed_headers(&["From"])
        .unwrap()
        .with_private_key(private_key)
        .with_selector("default")
        .with_signing_domain("zkemail.com")
        .with_logger(&logger)
        .with_header_canonicalization(cfdkim::canonicalization::Type::Relaxed)
        .with_body_canonicalization(cfdkim::canonicalization::Type::Relaxed)
        .build()
        .unwrap();
    let signature = signer.sign(&email).unwrap();
    println!("signature {}", signature);
    let new_msg = vec![signature.as_bytes(), b"\r\n", message].concat();
    let (canonicalized_header, canonicalized_body, signature_bytes) =
        canonicalize_signed_email(&new_msg).unwrap();

    let e = RSAPubE::Fix(BigUint::from(Bench1EmailVerifyCircuit::<Fr>::DEFAULT_E));
    let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
    let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
    let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
    let hash = Sha256::digest(&canonicalized_body);
    let mut expected_output = Vec::new();
    expected_output.resize(44, 0);
    BASE64_STANDARD
        .encode_slice(&hash, &mut expected_output)
        .unwrap();
    let substrings = vec![
        String::from_utf8(expected_output).unwrap(),
        "alice@zkemail.com".to_string(),
        "zkemailverify".to_string(),
    ];
    let circuit = Bench1EmailVerifyCircuit {
        header_bytes: canonicalized_header,
        body_bytes: canonicalized_body,
        public_key,
        signature,
        substrings,
    };
    let vk = keygen_vk(&params, &circuit).unwrap();
    let pk = keygen_pk(&params, vk, &circuit).unwrap();
    let instances = circuit.instances();
    group.bench_function("bench 1", |b| {
        b.iter(|| {
            let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
            create_proof::<KZGCommitmentScheme<_>, ProverGWC<_>, _, _, _, _>(
                &params,
                &pk,
                &vec![circuit.clone()],
                &[&instances.iter().map(|vec| &vec[..]).collect::<Vec<&[Fr]>>()[..]],
                OsRng,
                &mut transcript,
            )
            .unwrap();
            transcript.finalize();
        })
    });
    group.finish();
}

criterion_group!(benches, bench_email_verify1,);
criterion_main!(benches);
