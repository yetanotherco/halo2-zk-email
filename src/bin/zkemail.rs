use base64::prelude::{Engine as _, BASE64_STANDARD};
use cfdkim::{canonicalize_signed_email, resolve_public_key};
use clap::{Parser, Subcommand};
use fancy_regex::Regex;
use halo2_base::halo2_proofs::circuit::Value;
use halo2_base::halo2_proofs::halo2curves::bn256::{Bn256, Fr, G1Affine};
use halo2_base::halo2_proofs::plonk::{keygen_pk, keygen_vk, Error, ProvingKey, VerifyingKey};
use halo2_base::halo2_proofs::poly::commitment::Params;
use halo2_base::halo2_proofs::poly::kzg::commitment::ParamsKZG;
use halo2_base::halo2_proofs::SerdeFormat;
use halo2_rsa::{RSAPubE, RSAPublicKey, RSASignature};
use halo2_zk_email::*;
use hex;
use itertools::Itertools;
use num_bigint::BigUint;
use num_traits::Pow;
use rand::thread_rng;
use rsa::PublicKeyParts;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env::set_var;
use std::fmt::format;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use tokio::macros::*;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand, Clone)]
enum Commands {
    /// Generate a setup parameter (not for production).
    GenParam {
        /// k parameter for the one email verification circuit.
        #[arg(long)]
        k: u32,
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        param_path: String,
    },
    /// Downsize a setup parameter (not for production).
    DownsizeParam {
        /// k parameter for the one email verification circuit.
        #[arg(long)]
        k: u32,
        /// original setup parameters path
        #[arg(short, long, default_value = "./build/agg_params.bin")]
        original_param_path: String,
        /// downsized setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        new_param_path: String,
    },
    /// Generate a proving key and a verifying key.
    GenAppKey {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        circuit_config_path: String,
        /// emails path
        #[arg(short, long, default_value = "./build/demo.eml")]
        email_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/app.pk")]
        pk_path: String,
        /// verifying key file
        #[arg(long, default_value = "./build/app.vk")]
        vk_path: String,
    },
    /// Generate a proving key and a verifying key.
    GenAggKey {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        app_param_path: String,
        /// setup parameters path
        #[arg(short, long, default_value = "./build/agg_params.bin")]
        agg_param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        app_circuit_config_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_agg.config")]
        agg_circuit_config_path: String,
        /// emails path
        #[arg(short, long, default_value = "./build/demo.eml")]
        email_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/app.pk")]
        app_pk_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/agg.pk")]
        agg_pk_path: String,
        /// verifying key file
        #[arg(long, default_value = "./build/agg.vk")]
        agg_vk_path: String,
    },
    ProveApp {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        circuit_config_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/app.pk")]
        pk_path: String,
        /// emails path
        #[arg(short, long, default_value = "./build/demo.eml")]
        email_path: String,
        /// output proof file
        #[arg(long, default_value = "./build/app_proof.bin")]
        proof_path: String,
        /// public input file
        #[arg(long, default_value = "./build/public_input.json")]
        public_input_path: String,
    },
    EVMProveApp {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        circuit_config_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/app.pk")]
        pk_path: String,
        /// emails path
        #[arg(short, long, default_value = "./build/demo.eml")]
        email_path: String,
        /// output proof file
        #[arg(long, default_value = "./build/evm_app_proof.hex")]
        proof_path: String,
        /// public input file
        #[arg(long, default_value = "./build/public_input.json")]
        public_input_path: String,
    },
    EVMProveAgg {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        app_param_path: String,
        /// setup parameters path
        #[arg(short, long, default_value = "./build/agg_params.bin")]
        agg_param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        app_circuit_config_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_agg.config")]
        agg_circuit_config_path: String,
        /// emails path
        #[arg(short, long, default_value = "./build/demo.eml")]
        email_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/app.pk")]
        app_pk_path: String,
        /// proving key path
        #[arg(long, default_value = "./build/agg.pk")]
        agg_pk_path: String,
        /// output acc file
        #[arg(long, default_value = "./build/evm_agg_acc.hex")]
        acc_path: String,
        /// output proof file
        #[arg(long, default_value = "./build/evm_agg_proof.hex")]
        proof_path: String,
        /// public input file
        #[arg(long, default_value = "./build/public_input.json")]
        public_input_path: String,
    },
    GenEVMVerifier {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/app_params.bin")]
        param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        circuit_config_path: String,
        // /// emails path
        // #[arg(short, long, default_value = "./build/demo.eml")]
        // email_path: String,
        /// verifying key file
        #[arg(long, default_value = "./build/app.vk")]
        vk_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/verifier.bin")]
        bytecode_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/Verifier.sol")]
        solidity_path: String,
    },
    GenAggEVMVerifier {
        /// setup parameters path
        #[arg(short, long, default_value = "./build/agg_params.bin")]
        agg_param_path: String,
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        app_circuit_config_path: String,
        /// aggregation circuit configure file
        #[arg(short, long, default_value = "./configs/default_agg.config")]
        agg_circuit_config_path: String,
        // /// emails path
        // #[arg(short, long, default_value = "./build/demo.eml")]
        // email_path: String,
        /// verifying key file
        #[arg(long, default_value = "./build/agg.vk")]
        vk_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/verifier.bin")]
        bytecode_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/Verifier.sol")]
        solidity_path: String,
    },
    EVMVerifyApp {
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        circuit_config_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/verifier.bin")]
        bytecode_path: String,
        /// output proof file
        #[arg(long, default_value = "./build/evm_app_proof.hex")]
        proof_path: String,
        /// public input file
        #[arg(long, default_value = "./build/public_input.json")]
        public_input_path: String,
    },
    EVMVerifyAgg {
        /// email verification circuit configure file
        #[arg(short, long, default_value = "./configs/default_app.config")]
        app_circuit_config_path: String,
        /// aggregation circuit configure file
        #[arg(short, long, default_value = "./configs/default_agg.config")]
        agg_circuit_config_path: String,
        /// evm verifier file
        #[arg(short, long, default_value = "./build/verifier.bin")]
        bytecode_path: String,
        /// output proof file
        #[arg(long, default_value = "./build/evm_agg_proof.hex")]
        proof_path: String,
        /// output acc file
        #[arg(long, default_value = "./build/evm_agg_acc.hex")]
        acc_path: String,
        /// public input file
        #[arg(long, default_value = "./build/public_input.json")]
        public_input_path: String,
    },
    GenRegexFiles {
        #[arg(short, long, default_value = "./configs/decomposed_regex_config.json")]
        decomposed_regex_config_path: String,
        #[arg(long, default_value = "./build")]
        regex_dir_path: String,
        #[arg(short, long)]
        regex_files_prefix: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::GenParam { k, param_path } => gen_param(&param_path, k).unwrap(),
        Commands::DownsizeParam {
            k,
            original_param_path,
            new_param_path,
        } => downsize_param(&original_param_path, &new_param_path, k).unwrap(),
        Commands::GenAppKey {
            param_path,
            circuit_config_path,
            email_path,
            pk_path,
            vk_path,
        } => gen_app_key(&param_path, &circuit_config_path, &email_path, &pk_path, &vk_path).await.unwrap(),
        Commands::GenAggKey {
            app_param_path,
            agg_param_path,
            app_circuit_config_path,
            agg_circuit_config_path,
            email_path,
            app_pk_path,
            agg_pk_path,
            agg_vk_path,
        } => gen_agg_key(
            &app_param_path,
            &agg_param_path,
            &app_circuit_config_path,
            &agg_circuit_config_path,
            &email_path,
            &app_pk_path,
            &agg_pk_path,
            &agg_vk_path,
        )
        .await
        .unwrap(),
        Commands::ProveApp {
            param_path,
            circuit_config_path,
            pk_path,
            email_path,
            proof_path,
            public_input_path,
        } => prove_app(&param_path, &circuit_config_path, &pk_path, &email_path, &proof_path, &public_input_path)
            .await
            .unwrap(),
        Commands::EVMProveApp {
            param_path,
            circuit_config_path,
            pk_path,
            email_path,
            proof_path,
            public_input_path,
        } => evm_prove_app(&param_path, &circuit_config_path, &pk_path, &email_path, &proof_path, &public_input_path)
            .await
            .unwrap(),
        Commands::EVMProveAgg {
            app_param_path,
            agg_param_path,
            app_circuit_config_path,
            agg_circuit_config_path,
            email_path,
            app_pk_path,
            agg_pk_path,
            acc_path,
            proof_path,
            public_input_path,
        } => evm_prove_agg(
            &app_param_path,
            &agg_param_path,
            &app_circuit_config_path,
            &agg_circuit_config_path,
            &email_path,
            &app_pk_path,
            &agg_pk_path,
            &acc_path,
            &proof_path,
            &public_input_path,
        )
        .await
        .unwrap(),
        Commands::GenEVMVerifier {
            param_path,
            circuit_config_path,
            vk_path,
            bytecode_path,
            solidity_path,
        } => gen_evm_verifier(&param_path, &circuit_config_path, &vk_path, &bytecode_path, &solidity_path).await.unwrap(),
        Commands::GenAggEVMVerifier {
            agg_param_path,
            app_circuit_config_path,
            agg_circuit_config_path,
            vk_path,
            bytecode_path,
            solidity_path,
        } => gen_agg_evm_verifier(
            &agg_param_path,
            &app_circuit_config_path,
            &agg_circuit_config_path,
            &vk_path,
            &bytecode_path,
            &solidity_path,
        )
        .await
        .unwrap(),
        Commands::EVMVerifyApp {
            circuit_config_path,
            bytecode_path,
            proof_path,
            public_input_path,
        } => evm_verify_app(&circuit_config_path, &bytecode_path, &proof_path, &public_input_path).unwrap(),
        Commands::EVMVerifyAgg {
            app_circuit_config_path,
            agg_circuit_config_path,
            bytecode_path,
            proof_path,
            acc_path,
            public_input_path,
        } => evm_verify_agg(
            &app_circuit_config_path,
            &agg_circuit_config_path,
            &bytecode_path,
            &proof_path,
            &acc_path,
            &public_input_path,
        )
        .unwrap(),
        Commands::GenRegexFiles {
            decomposed_regex_config_path,
            regex_dir_path,
            regex_files_prefix,
        } => gen_regex_files(&decomposed_regex_config_path, &regex_dir_path, &regex_files_prefix).unwrap(),
    }
}
