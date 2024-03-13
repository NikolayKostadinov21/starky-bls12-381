use std::marker::PhantomData;

use crate::{
    fields::fq12_u64::exp_u64::{
        read_fq12_exp_u64_io, Fq12ExpU64IONative, Fq12ExpU64Stark, FQ12_EXP_U64_IO_LEN,
    },
    utils::utils::{get_u256_biguint, u16_columns_to_u32_columns_base_circuit},
};
use ark_bls12_381::{Fq, Fq12};
use ark_ff::Field;
use itertools::Itertools;
use plonky2::{
    field::extension::Extendable,
    hash::hash_types::RichField,
    iop::{
        generator::{GeneratedValues, SimpleGenerator},
        target::Target,
        witness::{PartialWitness, PartitionWitness, Witness, WitnessWrite},
    },
    plonk::{
        circuit_builder::CircuitBuilder,
        config::{AlgebraicHasher, GenericConfig},
    },
    util::{serialization::Buffer, timing::TimingTree},
};
use plonky2_bls12_381::fields::{fq12_target::Fq12Target, fq_target::FqTarget, native::MyFq12};
use starky::{
    proof::StarkProofWithPublicInputsTarget,
    prover::prove,
    recursive_verifier::{
        add_virtual_stark_proof_with_pis, set_stark_proof_with_pis_target,
        verify_stark_proof_circuit,
    },
    verifier::verify_stark_proof,
};

pub const FQ12_EXP_U64_INPUT_LEN: usize = 12 * 8 * 3 + 1;

#[derive(Clone, Debug)]
pub struct Fq12ExpU64Input {
    pub x: Fq12,
    pub offset: Fq12,
    pub exp_val: u64,
}

#[derive(Clone, Debug)]
pub struct Fq12ExpU64InputTarget<F: RichField + Extendable<D>, const D: usize> {
    pub x: Fq12Target<F, D>,
    pub offset: Fq12Target<F, D>,
    pub exp_val: Target,
}

impl<F: RichField + Extendable<D>, const D: usize> Fq12ExpU64InputTarget<F, D> {
    pub fn connect(builder: &mut CircuitBuilder<F, D>, lhs: &Self, rhs: &Self) {
        Fq12Target::connect(builder, &lhs.x, &rhs.x);
        Fq12Target::connect(builder, &lhs.offset, &rhs.offset);
        builder.connect(lhs.exp_val, rhs.exp_val);
    }
    pub fn to_vec(&self) -> Vec<Target> {
        self.x
            .to_vec()
            .iter()
            .chain(self.offset.to_vec().iter())
            .chain(&[self.exp_val])
            .cloned()
            .collect_vec()
    }
    pub fn from_vec(
        builder: &mut CircuitBuilder<F, D>,
        input: &[Target],
    ) -> Fq12ExpU64InputTarget<F, D> {
        assert!(input.len() == FQ12_EXP_U64_INPUT_LEN);
        let num_limbs = 8;
        let num_fq12_limbs = 12 * num_limbs;
        let mut input = input.to_vec();
        let x_raw = input.drain(0..num_fq12_limbs).collect_vec();
        let offset_raw = input.drain(0..num_fq12_limbs).collect_vec();
        let exp_val = input[0];
        assert_eq!(input.len(), 1);

        let x = Fq12Target::from_vec(builder, &x_raw);
        let offset = Fq12Target::from_vec(builder, &offset_raw);
        Fq12ExpU64InputTarget { x, offset, exp_val }
    }

    pub fn set_witness(&self, pw: &mut PartialWitness<F>, value: &Fq12ExpU64Input) {
        self.x.set_witness(pw, &value.x);
        self.offset.set_witness(pw, &value.offset);
        pw.set_target(self.exp_val, F::from_canonical_u64(value.exp_val));
    }
}

#[derive(Clone, Debug)]
pub struct Fq12ExpU64OutputGenerator<F: RichField + Extendable<D>, const D: usize> {
    pub input: Fq12ExpU64InputTarget<F, D>,
    pub output: Fq12Target<F, D>,
}

impl<F, const D: usize> SimpleGenerator<F> for Fq12ExpU64OutputGenerator<F, D>
where
    F: RichField + Extendable<D>,
{
    fn dependencies(&self) -> Vec<Target> {
        self.input.to_vec()
    }

    fn run_once(&self, pw: &PartitionWitness<F>, out_buffer: &mut GeneratedValues<F>) {
        let x_coeffs: [Fq; 12] = self
            .input
            .x
            .clone()
            .coeffs
            .map(|x| get_u256_biguint(pw, &x.to_vec()).into());
        let x: Fq12 = MyFq12 { coeffs: x_coeffs }.into();
        let offset_coeffs = self
            .input
            .offset
            .clone()
            .coeffs
            .map(|x| get_u256_biguint(pw, &x.to_vec()).into());
        let offset: Fq12 = MyFq12 {
            coeffs: offset_coeffs,
        }
        .into();
        let exp_val = pw.get_target(self.input.exp_val).to_canonical_u64();
        let output = offset * x.pow(&[exp_val]);
        self.output.set_witness(out_buffer, &output);
    }

    fn id(&self) -> String {
        "Fq12ExpU64OutputGenerator".to_string()
    }
    fn serialize(&self, _dst: &mut Vec<u8>) -> plonky2::util::serialization::IoResult<()> {
        todo!()
    }
    fn deserialize(_src: &mut Buffer) -> plonky2::util::serialization::IoResult<Self> {
        todo!()
    }
}

#[derive(Clone, Debug)]
pub struct Fq12ExpU64StarkyProofGenerator<
    F: RichField + Extendable<D>,
    C: GenericConfig<D, F = F>,
    const D: usize,
> {
    pub inputs: Vec<Fq12ExpU64InputTarget<F, D>>,
    pub outputs: Vec<Fq12Target<F, D>>,
    pub starky_proof: StarkProofWithPublicInputsTarget<D>,
    _config: std::marker::PhantomData<C>,
}

impl<F, C, const D: usize> SimpleGenerator<F> for Fq12ExpU64StarkyProofGenerator<F, C, D>
where
    F: RichField + Extendable<D>,
    C: GenericConfig<D, F = F> + 'static,
    C::Hasher: AlgebraicHasher<F>,
{
    fn dependencies(&self) -> Vec<Target> {
        let mut targets = vec![];
        self.inputs.iter().cloned().for_each(|input| {
            targets.extend(input.to_vec());
        });
        targets
    }
    fn run_once(&self, pw: &PartitionWitness<F>, out_buffer: &mut GeneratedValues<F>) {
        let ios_native = self
            .inputs
            .iter()
            .cloned()
            .map(|input| {
                let x_coeffs: [Fq; 12] = input
                    .x
                    .coeffs
                    .map(|x| get_u256_biguint(pw, &x.to_vec()).into());
                let x: Fq12 = MyFq12 { coeffs: x_coeffs }.into();
                let offset_coeffs = input
                    .offset
                    .coeffs
                    .map(|x| get_u256_biguint(pw, &x.to_vec()).into());
                let offset: Fq12 = MyFq12 {
                    coeffs: offset_coeffs,
                }
                .into();
                let exp_val = pw.get_target(input.exp_val).to_canonical_u64();
                let output = offset * x.pow(&[exp_val]);
                Fq12ExpU64IONative {
                    x,
                    offset,
                    exp_val,
                    output,
                }
            })
            .collect_vec();

        let num_io = ios_native.len();
        let stark = Fq12ExpU64Stark::<F, D>::new(num_io);
        let inner_config = stark.config();
        let trace = stark.generate_trace(&ios_native);
        let pi = stark.generate_public_inputs(&ios_native);
        let inner_proof = prove::<F, C, _, D>(
            stark,
            &inner_config,
            trace,
            pi.try_into().unwrap(),
            &mut TimingTree::default(),
        )
        .unwrap();
        verify_stark_proof(stark, inner_proof.clone(), &inner_config).unwrap();
        set_stark_proof_with_pis_target(out_buffer, &self.starky_proof, &inner_proof);
    }

    fn id(&self) -> String {
        "Fq12ExpU64StarkyProofGenerator".to_string()
    }
    fn serialize(&self, _dst: &mut Vec<u8>) -> plonky2::util::serialization::IoResult<()> {
        todo!()
    }
    fn deserialize(_src: &mut Buffer) -> plonky2::util::serialization::IoResult<Self> {
        todo!()
    }
}

fn fq12_exp_u64_circuit_with_proof_target<
    F: RichField + Extendable<D>,
    C: GenericConfig<D, F = F>,
    const D: usize,
>(
    builder: &mut CircuitBuilder<F, D>,
    log_num_io: usize,
) -> (
    Vec<Fq12ExpU64InputTarget<F, D>>,
    Vec<Fq12Target<F, D>>,
    StarkProofWithPublicInputsTarget<D>,
)
where
    <C as GenericConfig<D>>::Hasher: AlgebraicHasher<F>,
{
    let num_io = 1 << log_num_io;
    let stark = Fq12ExpU64Stark::<F, D>::new(num_io);
    let inner_config = stark.config();
    let degree_bits = 7 + log_num_io;
    let starky_proof_t =
        add_virtual_stark_proof_with_pis(builder, stark, &inner_config, degree_bits);
    verify_stark_proof_circuit::<F, C, _, D>(builder, stark, &starky_proof_t, &inner_config);
    assert!(starky_proof_t.public_inputs.len() == FQ12_EXP_U64_IO_LEN * num_io);
    let mut cur_col = 0;
    let mut inputs = vec![];
    let mut outputs = vec![];
    let pi = starky_proof_t.public_inputs.clone();
    for _ in 0..num_io {
        let io = read_fq12_exp_u64_io(&pi, &mut cur_col);
        let x_coeffs =
            io.x.iter()
                .map(|limb| {
                    // range check
                    limb.iter().for_each(|l| builder.range_check(*l, 16));
                    let limb_u32 = u16_columns_to_u32_columns_base_circuit(builder, *limb);
                    FqTarget::from_limbs(builder, &limb_u32)
                })
                .collect_vec();
        let offset_coeffs = io
            .offset
            .iter()
            .map(|limb| {
                // range check
                limb.iter().for_each(|l| builder.range_check(*l, 16));
                let limb_u32 = u16_columns_to_u32_columns_base_circuit(builder, *limb);
                FqTarget::from_limbs(builder, &limb_u32)
            })
            .collect_vec();
        let output_coeffs = io
            .output
            .iter()
            .map(|limb| {
                // range check
                // limb.iter().for_each(|l| builder.range_check(*l, 16));
                let limb_u32 = u16_columns_to_u32_columns_base_circuit(builder, *limb);
                FqTarget::from_limbs(builder, &limb_u32)
            })
            .collect_vec();
        let x = Fq12Target::new(x_coeffs);
        let offset = Fq12Target::new(offset_coeffs);
        let output = Fq12Target::new(output_coeffs);
        let exp_val = io.exp_val;
        let input = Fq12ExpU64InputTarget { x, offset, exp_val };
        inputs.push(input);
        outputs.push(output);
    }
    (inputs, outputs, starky_proof_t)
}

pub fn fq12_exp_u64_circuit<
    F: RichField + Extendable<D>,
    C: GenericConfig<D, F = F> + 'static,
    const D: usize,
>(
    builder: &mut CircuitBuilder<F, D>,
    inputs: &[Fq12ExpU64InputTarget<F, D>],
) -> Vec<Fq12Target<F, D>>
where
    C::Hasher: AlgebraicHasher<F>,
{
    let n = inputs.len();
    let next_power_of_two = n.next_power_of_two();
    let mut inputs = inputs.to_vec();
    inputs.resize(next_power_of_two, inputs.last().unwrap().clone());
    let log_num_io = next_power_of_two.trailing_zeros() as usize;

    // ok
    let (inputs_constr, outputs, starky_proof) =
        fq12_exp_u64_circuit_with_proof_target::<F, C, D>(builder, log_num_io);

    for (input_c, input) in inputs_constr.iter().zip(inputs.iter()) {
        Fq12ExpU64InputTarget::connect(builder, input_c, input);
    }

    for (input, output) in inputs.iter().zip(outputs.iter()) {
        let output_generator = Fq12ExpU64OutputGenerator {
            input: input.to_owned(),
            output: output.to_owned(),
        };
        builder.add_simple_generator(output_generator);
    }

    let proof_generator = Fq12ExpU64StarkyProofGenerator::<F, C, D> {
        inputs: inputs.to_vec(),
        outputs: outputs.clone(),
        starky_proof,
        _config: PhantomData,
    };
    builder.add_simple_generator(proof_generator);
    outputs[..n].to_vec()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ark_bls12_381::Fq12;
    use ark_ff::Field;
    use ark_std::UniformRand;
    use itertools::Itertools;
    use num_traits::One;
    use plonky2::{
        field::{
            goldilocks_field::GoldilocksField,
            types::{PrimeField64, Sample},
        },
        iop::witness::{PartialWitness, WitnessWrite},
        plonk::{
            circuit_builder::CircuitBuilder,
            circuit_data::CircuitConfig,
            config::{GenericConfig, PoseidonGoldilocksConfig},
        },
        util::timing::TimingTree,
    };
    use plonky2_bls12_381::fields::fq12_target::Fq12Target;
    use starky::{
        prover::prove, recursive_verifier::set_stark_proof_with_pis_target,
        verifier::verify_stark_proof,
    };

    use crate::fields::fq12_u64::{
        circuit::{
            fq12_exp_u64_circuit, fq12_exp_u64_circuit_with_proof_target, Fq12ExpU64Input,
            Fq12ExpU64InputTarget,
        },
        exp_u64::{Fq12ExpU64IONative, Fq12ExpU64Stark},
    };

    #[test]
    fn test_fq12_exp_u64_circuit() {
        let log_num_io = 1;
        let num_io = 1 << log_num_io;
        let mut rng = rand::thread_rng();
        const D: usize = 2;
        type C = PoseidonGoldilocksConfig;
        type F = <C as GenericConfig<D>>::F;

        let circuit_config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(circuit_config);
        let (inputs_t, outputs_t, starky_proof_t) =
            fq12_exp_u64_circuit_with_proof_target::<F, C, D>(&mut builder, log_num_io);
        dbg!(starky_proof_t.public_inputs.len());

        let stark = Fq12ExpU64Stark::<F, D>::new(num_io);
        let inner_config = stark.config();

        let mut ios = vec![];
        let mut inputs = vec![];
        let mut outputs = vec![];
        for _ in 0..num_io {
            let exp_val_f = F::sample(&mut rng);
            let exp_val = exp_val_f.to_canonical_u64();
            let x = Fq12::rand(&mut rng);
            let offset = Fq12::rand(&mut rng);
            let output: Fq12 = offset * x.pow(&[exp_val]);
            let input = Fq12ExpU64Input { x, offset, exp_val };
            let io = Fq12ExpU64IONative {
                x,
                offset,
                exp_val,
                output,
            };
            inputs.push(input);
            outputs.push(output);
            ios.push(io);
        }

        let trace = stark.generate_trace(&ios);
        let pi = stark.generate_public_inputs(&ios);
        dbg!(pi.len());
        let inner_proof =
            prove::<F, C, _, D>(stark, &inner_config, trace, pi, &mut TimingTree::default())
                .unwrap();
        verify_stark_proof(stark, inner_proof.clone(), &inner_config).unwrap();

        dbg!(builder.num_gates());
        let data = builder.build::<C>();

        let mut pw = PartialWitness::<F>::new();
        println!("before set_stark_proof_with_pis_target");
        set_stark_proof_with_pis_target(&mut pw, &starky_proof_t, &inner_proof);
        println!("after set_stark_proof_with_pis_target");
        inputs_t.iter().zip(inputs.iter()).for_each(|(t, w)| {
            t.set_witness(&mut pw, w);
        });
        outputs_t.iter().zip(outputs.iter()).for_each(|(t, w)| {
            t.set_witness(&mut pw, w);
        });
        let now = Instant::now();
        let _proof = data.prove(pw).unwrap();
        println!("end plonky2 proof generation: {:?}", now.elapsed());
    }

    #[test]
    fn test_fq12_u64_msm() {
        let mut rng = rand::thread_rng();
        type F = GoldilocksField;
        type C = PoseidonGoldilocksConfig;
        const D: usize = 2;
        let log_num_io = 2;
        let num_io = 1 << log_num_io;

        let config = CircuitConfig::standard_ecc_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);
        let inputs_t = (0..num_io)
            .map(|_| {
                let x = Fq12Target::empty(&mut builder);
                let offset = Fq12Target::empty(&mut builder);
                let exp_val = builder.add_virtual_target();
                Fq12ExpU64InputTarget { x, offset, exp_val }
            })
            .collect_vec();
        let outputs_t = fq12_exp_u64_circuit::<F, C, D>(&mut builder, &inputs_t);

        let one = Fq12Target::constant(&mut builder, Fq12::one());
        Fq12Target::connect(&mut builder, &inputs_t[0].offset, &one);
        for i in 1..num_io {
            Fq12Target::connect(&mut builder, &inputs_t[i].offset, &outputs_t[i - 1]);
        }
        let data = builder.build::<C>();

        let now = Instant::now();
        let mut pw = PartialWitness::<F>::new();
        let xs = (0..num_io).map(|_| Fq12::rand(&mut rng)).collect_vec();
        let exp_vals: Vec<u64> = (0..num_io)
            .map(|_| {
                let exp_val_f = F::sample(&mut rng);
                exp_val_f.to_canonical_u64()
            })
            .collect_vec();
        let expected = {
            let mut total = Fq12::one();
            for i in 0..num_io {
                total = total * xs[i].pow(&[exp_vals[i]]);
            }
            total
        };
        use plonky2::field::types::Field;
        for i in 0..num_io {
            inputs_t[i].x.set_witness(&mut pw, &xs[i]);
            pw.set_target(inputs_t[i].exp_val, F::from_canonical_u64(exp_vals[i]));
        }
        let output = outputs_t.last().unwrap();
        output.set_witness(&mut pw, &expected);
        let _proof = data.prove(pw).unwrap();
        println!("Fq12 u64 msm time: {:?}", now.elapsed());
    }
}
