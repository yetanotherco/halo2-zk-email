use crate::wtns_commit::{
    assigned_commit_wtns_bytes,
    poseidon_circuit::{HasherChip, PoseidonChipBn254_8_58},
    value_commit_wtns_bytes,
};
use crate::*;
use halo2_base::halo2_proofs::plonk::ConstraintSystem;
use halo2_base::halo2_proofs::{
    circuit::{Cell, Layouter, SimpleFloorPlanner},
    plonk::{Circuit, Column, Error, Instance},
};
use halo2_base::{
    gates::range::{RangeConfig, RangeStrategy::Vertical},
    utils::PrimeField,
    SKIP_FIRST_PASS,
};
use halo2_dynamic_sha256::Sha256DynamicConfig;
use sha2::{self, Digest, Sha256};
use snark_verifier_sdk::CircuitExt;

#[derive(Debug, Clone)]
struct Sha256InstanceConfig<F: PrimeField> {
    inner: Sha256DynamicConfig<F>,
    instance: Column<Instance>,
}

#[macro_export]
macro_rules! impl_sha2_circuit {
    ($circuit_name:ident, $max_bytes_size:expr, $num_flex_advice:expr, $num_flex_fixed:expr, $num_range_lookup_advice:expr, $range_lookup_bits:expr, $degree:expr, $sha2_num_bits_lookup:expr, $sha2_num_advice_columns:expr, $skip_prefix_bytes_size:expr) => {
        #[derive(Debug, Clone)]
        struct $circuit_name<F: PrimeField> {
            input: Vec<u8>,
            sign_rand: F,
        }

        impl<F: PrimeField> Circuit<F> for $circuit_name<F> {
            type Config = Sha256InstanceConfig<F>;
            type FloorPlanner = SimpleFloorPlanner;

            fn without_witnesses(&self) -> Self {
                Self {
                    input: vec![0; self.input.len()],
                    sign_rand: F::zero(),
                }
            }

            fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
                // let config_params = read_default_circuit_config_params();
                // let sha256_params = config_params.sha256_config.unwrap();
                let range_config = RangeConfig::configure(
                    meta,
                    Vertical,
                    &[$num_flex_advice],
                    &[$num_range_lookup_advice],
                    $num_flex_fixed,
                    $range_lookup_bits,
                    0,
                    $degree,
                );
                let inner = Sha256DynamicConfig::configure(
                    meta,
                    vec![$max_bytes_size],
                    range_config.clone(),
                    $sha2_num_bits_lookup,
                    $sha2_num_advice_columns,
                    false,
                );
                let instance = meta.instance_column();
                meta.enable_equality(instance);
                Self::Config { inner, instance }
            }

            fn synthesize(&self, mut config: Self::Config, mut layouter: impl Layouter<F>) -> Result<(), Error> {
                config.inner.range().load_lookup_table(&mut layouter)?;
                config.inner.load(&mut layouter)?;
                let mut first_pass = SKIP_FIRST_PASS;
                let mut public_input_cells = vec![];
                layouter.assign_region(
                    || "sha2",
                    |region| {
                        if first_pass {
                            first_pass = false;
                            return Ok(());
                        }
                        let ctx = &mut config.inner.new_context(region);
                        let assigned_hash_result = config.inner.digest(ctx, &self.input, Some($skip_prefix_bytes_size))?;
                        let range = config.inner.range();
                        let gate = range.gate();
                        let poseidon = PoseidonChipBn254_8_58::new(ctx, gate);
                        let sign_rand = gate.load_witness(ctx, Value::known(self.sign_rand));
                        let hash_commit = assigned_commit_wtns_bytes(ctx, gate, &poseidon, &sign_rand, &assigned_hash_result.output_bytes);
                        let mut is_input_revealed = gate.load_constant(ctx, F::one());
                        let mut actual_input = vec![];
                        let expected_len = gate.sub(
                            ctx,
                            QuantumCell::Existing(&assigned_hash_result.input_len),
                            QuantumCell::Constant(F::from($skip_prefix_bytes_size as u64)),
                        );
                        for (idx, assigned_byte) in assigned_hash_result.input_bytes.iter().enumerate() {
                            let is_len_equal = gate.is_equal(ctx, QuantumCell::Existing(&expected_len), QuantumCell::Constant(F::from(idx as u64)));
                            is_input_revealed = gate.select(
                                ctx,
                                QuantumCell::Constant(F::zero()),
                                QuantumCell::Existing(&is_input_revealed),
                                QuantumCell::Existing(&is_len_equal),
                            );
                            let assigned_byte = gate.mul(ctx, QuantumCell::Existing(&assigned_byte), QuantumCell::Existing(&is_input_revealed));
                            actual_input.push(assigned_byte);
                        }
                        let input_commit = assigned_commit_wtns_bytes(ctx, gate, &poseidon, &sign_rand, &actual_input);
                        public_input_cells.push(input_commit.cell());
                        public_input_cells.push(hash_commit.cell());
                        config.inner.range().finalize(ctx);
                        Ok(())
                    },
                )?;
                for (idx, cell) in public_input_cells.into_iter().enumerate() {
                    layouter.constrain_instance(cell, config.instance, idx)?;
                }
                Ok(())
            }
        }

        impl<F: PrimeField> CircuitExt<F> for $circuit_name<F> {
            fn num_instance(&self) -> Vec<usize> {
                vec![2]
            }

            fn instances(&self) -> Vec<Vec<F>> {
                let padding_size = $max_bytes_size - self.input.len();
                let input_bytes = vec![&self.input[..], &vec![0; padding_size]].concat();
                let input_commit = value_commit_wtns_bytes(&self.sign_rand, &input_bytes);
                let hash_commit = value_commit_wtns_bytes(&self.sign_rand, &Sha256::digest(&self.input).to_vec());
                vec![vec![input_commit, hash_commit]]
            }
        }

        impl<F: PrimeField> $circuit_name<F> {
            pub fn new(input: Vec<u8>, sign_rand: F) -> Self {
                Self { input, sign_rand }
            }
        }
    };
}

impl_sha2_circuit!(DummySha256Circuit, 128, 1, 0, 1, 8, 2, 8, 1, 0);
