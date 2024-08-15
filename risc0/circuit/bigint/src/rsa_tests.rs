// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::borrow::Borrow;

use anyhow::Result;
use num_bigint::BigUint;
use num_traits::Num;
use pretty_assertions::assert_eq;
use risc0_circuit_bigint_test_methods::{RSA_ELF, RSA_ID};
use risc0_circuit_recursion::{prove::Prover, CHECKED_COEFFS_PER_POLY};
use risc0_zkp::core::hash::{poseidon2::Poseidon2HashSuite, sha};
use risc0_zkp::field::{
    baby_bear::{BabyBearElem, BabyBearExtElem},
    Elem, ExtElem,
};
use risc0_zkvm::{default_prover, ExecutorEnv};
use test_log::test;
use tracing::trace;

use crate::{
    byte_poly::BytePoly,
    prove,
    rsa::{RSA_256_X1, RSA_256_X2},
    verify,
    zkr::get_zkr,
    BigIntContext, BIGINT_PO2,
};

fn from_hex(s: &str) -> BigUint {
    BigUint::from_str_radix(s, 16).expect("Unable to parse hex value")
}

// "golden" values are the values from running the C++ version:
// bazelisk run //zirgen/Dialect/BigInt/IR/test:test -- --test

fn golden_values() -> Vec<BigUint> {
    Vec::from([
        from_hex("9c98f9aacfc0b73c916a824db9afe39673dcb56c42dffe9de5b86d5748aca4d5"),
        from_hex("de67116c809a5cc876cebb5e8c72d998f983a4d61b499dd9ae23b789a7183677"),
        from_hex("1fb897fac8aa8870b936631d3af1a17930c8af0ca4376b3056677ded52adf5aa"),
    ])
}

fn golden_z() -> BabyBearExtElem {
    BabyBearExtElem::from_subelems(
        [1900860849, 639441699, 1578171302, 1926064223]
            .into_iter()
            .map(BabyBearElem::from_u64),
    )
}

fn witness_test_data(data: &[&str]) -> Vec<BytePoly> {
    data.iter().map(|d| BytePoly::from_hex(d)).collect()
}

fn golden_constant_witness() -> Vec<BytePoly> {
    witness_test_data(&[])
}

fn golden_public_witness() -> Vec<BytePoly> {
    witness_test_data(&[
        "d5a4ac48576db8e59dfedf426cb5dc7396e3afb94d826a913cb7c0cfaaf9989c",
        "773618a789b723aed99d491bd6a483f998d9728c5ebbce76c85c9a806c1167de",
        "aaf5ad52ed7d6756306b37a40cafc83079a1f13a1d6336b97088aac8fa97b81f",
    ])
}

fn golden_private_witness() -> Vec<BytePoly> {
    witness_test_data(&[
        "daba7f9e422e98ce0e2c194e9e24fb08f6375bceb4ea4a158520c33d7b2fdc3b0100",
  "ef4b74f54a1f24c99c152a56b7bcf650d512b33bc88b206f47e8555fa081f87e",
  "067cda7330ff9ff61bc48b3a7b0345873c38c2127d2ec3bd55a4ccad4b30e59f4a86b587d981047964e0337d2472a7fffc058cdfdc544cde3718b0231a7feb878787",
  "383838383837383838383838373839373837373837383637363637363837373637373536353736353636373637373637363837373637373738383738383736373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "3cdee9afe68426a7f1c1311ed4e77605b454b9a67078a2f9070c4d7f2af3f2660000",
  "3591d9fcaf2f4df63c7ce9e8b7e488bc274b3eea1aa5737d20ee5d5edd689d16",
  "dada124858621e031b50a5adc1f514cf7117863da4725f3b058318eb4e61e33d99862e5195b54dc1d98c078cbe0e057ecc959b8399969afbc92ff9c99f6387878787",
  "3637383738383838393a393939393938383a3a3a39393b3a3a393a39383b3a3b3939393b3a3939393937383737383839383837373837373637383737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "847dd69d8904b444138a455c52062ff33ce82c8e987371fed838ccf7641244030000",
  "2516a8a55ae0a444fb54f8c50861ac345208d198ee207c0a918a3d023ab58693",
  "ea0837af20660e5783b231ea70211e98d3056d427752449e11db6b8769f476bd4cbcd78df2e6a04519d33294c5c7c14b0998ab7f3787d110f306516d4b9787878787",
  "373838373736373737373736363534353536363736363536373636383737373736383737373736373737383838393938393838383938383837383737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "75d4869fb6a5ae49225415cf6c54a95fd8d6399e192f3e52c5df7a0de8f6fa8a0000",
  "00a89402ad6e20334cb0e986102737edecbf5f18b679dd2dbc07f8dcf0e3ab44",
  "e37d9cc59f68cf7df29eea594b697a868fad5b6064ab706d87db077469e66d89579b0da8fdf7386fa9e9305c3fef47baabfed62e1ea1c9bef7f861f723d887878787",
  "37383838383838383738373838373838373939393938383938393a39393a3b3c3b3a3a3a393a3a39393a3b3b3a3a3b3a393738393a39393939393938383737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "94da09278887f38e5521ab3ccfbe6bd45806fda6fca57d07ca60a73c602a1d1e0000",
  "dc529667436f9ffa78e1e0367c7ec4ee4d30ee85e304d68ffa864a6cc0ea838a",
  "039c11a82f1081a279f0636ffa467fb30743cfd6c38979f15764a7f2af2fd4af12eaf89b50d8ffefca7cfe063b879090187fcda0ed43cfcb3462b85ae34f87878787",
  "383838373838383939383938383a3a3a3a3939393a3a3b3a3a39393a3a3b383738383939383636363738363735363537363735363536353535353536363737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "26e9e2e98e22d70dd813be95cb15d6b6ec3fbeb5a7624bd40f28e2b5d05f857a0000",
  "7258f1ef6ac719f1631ab74f0fe62fa2f5ca9723afe75a04e4f7df845a6ea149",
  "ead4d7716dd9ec096727fda13fd64b4e0e3b52642acf1532b9f917d293a896c0f1248afc2870118986dcac4e2fe8714402034f810b7978a736307a52479387878787",
  "363737383837373738373738383737373838383838383738363638373837373737383637373735373738363737383737373737363736373738383737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "ca366ba3bf9ccd7642594a55e9b76a95962c7186d3f65660b2e1ca31e0cf9e220000",
  "b2942c3ad93c5dab3d84e2a316d99ebb60145fe9f175ef8cb1c58bde89f39624",
  "fde8963645bbca2872d5367b72179f09ace4bde32e0c6726ad223dbc8adc21716db81e3fa68ba2de9daff4d1b39f336191b8d7c137a853534daa8d7ee1a087878787",
  "3737373737363737373737383839393838373638373939373739383737383a383838393a393839393837373838373738383837373838383838383838373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "683d1ba75f91f848d15c7c4c1fc2f45777915c25b9d40cf173d3c94347a08c080000",
  "3c94ad1c5a6f7652be1afa58ee0a967fe9c9b930873004fc0ec3cf2291613963",
  "622f78e6109371dc177efe21ce72d47adc74e7c868b7591738c13651f7c9710ffee51a5a5c3e43d2d32b78bb3b359d0ebae9fe3dbe9df1431620135ba7b787878787",
  "373737373737383738383737383837383838383737383838373737383635373837363737373736363536363636363636353435363636363737373737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "b3d056e96baf366e07fb81e76f2d4168ba085ba1539b09f79e45b7bfe7f3de3e0000",
  "211dbb7b2e451776e08b13fd893cf67911961ba511ed49490cfab7becb7ec06e",
  "0e62276a68e471eb8987a9f2d151e983cf3d51cd4dfec6264e6a0feeebab645d333fb24ef7b1c822fbe0c865f2fc15994b6a92b6f9f03b3cacb1b90c850887878787",
  "38383838383838383839393938393839383938393839373a38393938393b3b3a3b393a393a3a3a3a393839383939393a39393a383939393a39393939383837373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "1dc623821d7c19412c5386eecd8099a8d0a66a517cc307cbb08c902717ff534e0000",
  "20145cd114402e3fa79d1bdab0f73a7fedff1edc0cec6f57945024b1f02ed267",
  "9b37044e9ce2055867341e84498a912f4274a53c2e6c2f602345dda1241ba634b39d67430e34e7af957d3d20670ed64948d7ef4f8be583a865b00845a34387878787",
  "3738383837373838383838383938383938393a393a3a39393b3a3a3a393a3a3b39383939383938383839383a38393838383737373736373636363737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "8d0b1a80cc8b6926f91760093dd0ac0ab0b5a802b916b988920fa501e6ccd4440000",
  "af13c70819f13f54697e6568d7e45e3d0e4890be622a50f8463a9067ef921f1f",
  "f9e6ebef81738ad90e3b7e31e2c33cc245c793dd4cd011180f1b1ca6a2366067e6fa7f6fedef0f2f5a0be4925673aa0a94a3272778ea3232659d7b7bf38887878787",
  "37373737383838383939393838393938383939373838393a373839383739383a37373839373837383637373737383738383738383738383838383738373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "bfadf796682006dd9107492798ad5661696c01b14a7d1769e62eeac792882f060000",
  "b685dfcf4cf45a8fd7e15eca4dafbee41aa82640d8f0f155b6c772b8eec9d54d",
  "af78342599c39746b1b900f29093ded9179e8e3a521c6d68dc9e97bf1b92afa4d4380d4a14406ef9962311f72e716d376f87d8ed8e4bd21362535a06d5a087878787",
  "373838393837383839393938383838383a393839393a393838383a3a393b3a39393a3a3939393938383839393a393838383838373839383938383838373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "0c431789231067bd1ed57b599d99de71fd8671891f687635a405b3e07ce5af260000",
  "68248c2982d4c3a55aaab812e606785f9732f70f7c84dd3e81f33eb3d464e716",
  "10094b291c16a7a0f76beb8f4b0add148e41e1975d46eb06af2be3671079d7c1f346f297b4b118e0184da6e99db8e7d001076ad08577850444473240758887878787",
  "373736363635353534353434353534353535353635363335343434343433323333343434353536353736353636363636363534343536363737373737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "29d6fa6fbb5bd197ee81dbdba2f11455c99ae0532abb5cb4d6c89cb2bd9359030000",
  "23f67ade9dfd859ceccca28d2f4884ea8a8ef92993bee41a7c2b4d09b5d84466",
  "7f3786d89c3a6c7e7e0a73d80f7b25620bee3ba224171f9367034c1bdb483858f77b464c1d75456a39378e67baa44efadc2793529b7ca7b16fc16cd1379787878787",
  "373838383838383839393939393a393a3a3a3b3b3b3b3a3b3a3c3b3b3a3c3a3c393b393b3a3a3b3a393b393939383838373837383738383738373737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "29ac8406ec0a629695ec55599bc519d00cba9ed1f7bd94179da81510c9e2c9420000",
  "acc6c99bd59f325123ec88bd277dd75ce9d861e3b370e029252a827842867354",
  "a5ee7239d755b13909f0a5ad113479b9a3885df7f8e4b6da39f90dc6d1f65c677b7a1559651439d63c0baa69f37e05203345b0e59390324e335982750bf387878787",
  "37373737363635363635353535363636353636353636363436353737383738383737393938393a38393a39393839393a3a3938383838393939383838383737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "4b85da790c486de69dae77d07af2d9444f73ef0b2d97f039a54960fc58298b2d0000",
  "2990add598b7e3b53a05520d022c8cecb9d3036a42d092a19a61fce8e89b9149",
  "521c1ddf04930c3207d29554a392901ec777f20b79beddfed9c6dd074e7219080b9789f43730f4488233bf2cf27ac87f047671b8115416eda8552ed793ab87878787",
  "373737363635363738373737373737383737363736363636373737393938383839373938393a38393939383938383738383838383939393838383837373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "515cd4fe6aacff31634b7e2fea76b0467e2afec0e124e736d8b61931abd27b680000",
  "aaf5ad52ed7d6756306b37a40cafc83079a1f13a1d6336b97088aac8fa97b81f",
  "b8bd3788501f100b2e2b296cd9521be45b7bd670c45c21bbd48f43ea485a59e993268b431d0518daf135aebc6c7757fd9b5087b6bd8d7bea0d9aeaf9f07587878787",
  "373738383838383838383838393939383938383939393938383939383939393939393839383938383738373737373737373837373637373738373737373737373737",
  "040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404040404",
  "101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010",
  "2222222222222222222222222222222222222222222222222222222222222222",


    ])
}

fn run_bigint() -> Result<BigIntContext> {
    let mut ctx = BigIntContext {
        in_values: golden_values(),
        ..Default::default()
    };
    crate::generated::rsa_256_x1(&mut ctx)?;
    Ok(ctx)
}

#[test]
fn test_witgen() -> anyhow::Result<()> {
    let ctx = run_bigint()?;

    assert_eq!(ctx.public_witness, golden_public_witness());
    assert_eq!(ctx.private_witness, golden_private_witness());
    assert_eq!(ctx.constant_witness, golden_constant_witness());

    let hash_suite = Poseidon2HashSuite::new_suite();

    let public_digest = BytePoly::compute_digest(&*hash_suite.hashfn, &ctx.public_witness, 1);
    trace!("public_digest: {public_digest}");
    let private_digest = BytePoly::compute_digest(&*hash_suite.hashfn, &ctx.private_witness, 3);
    trace!("private_digest: {private_digest}");
    let folded = hash_suite.hashfn.hash_pair(&public_digest, &private_digest);
    trace!("folded: {folded}");

    let mut rng = hash_suite.rng.new_rng();
    rng.mix(&folded);
    let z = rng.random_ext_elem();
    assert_eq!(z, golden_z());

    Ok(())
}

#[test]
fn test_zkr() {
    let ctx = run_bigint().unwrap();

    let hash_suite = Poseidon2HashSuite::new_suite();

    let mut all_coeffs: Vec<u32> = Vec::new();
    for witness in ctx
        .constant_witness
        .iter()
        .chain(ctx.public_witness.iter())
        .chain(ctx.private_witness.iter())
    {
        for chunk in witness.chunks(CHECKED_COEFFS_PER_POLY) {
            let mut bytes: Vec<u8> = chunk
                .iter()
                .map(|b| u8::try_from(*b).expect("Byte out of range in witness coeffs"))
                .collect();
            while bytes.len() < CHECKED_COEFFS_PER_POLY {
                bytes.push(0);
            }

            for word in bytes.chunks(4) {
                all_coeffs.push(u32::from_le_bytes(
                    word.try_into().expect("Partial word present in witness?"),
                ));
            }
        }
    }

    let public_digest = BytePoly::compute_digest(&*hash_suite.hashfn, &ctx.public_witness, 1);
    trace!("public_digest: {public_digest}");
    let private_digest = BytePoly::compute_digest(&*hash_suite.hashfn, &ctx.private_witness, 3);
    trace!("private_digest: {private_digest}");
    let folded = hash_suite.hashfn.hash_pair(&public_digest, &private_digest);
    trace!("folded: {folded}");

    let mut rng = hash_suite.rng.new_rng();
    rng.mix(&folded);
    let z = rng.random_ext_elem();

    let program = get_zkr("rsa_256_x1.zkr", /*po2=*/ 12).unwrap();

    let mut prover = Prover::new(program, "poseidon2");
    prover.add_input(&[0u32; 8]); //control id
    for _ in 0..RSA_256_X1.iters {
        prover.add_input(&z.to_u32_words());
        prover.add_input(&all_coeffs);
    }
    let receipt = prover.run().unwrap();

    trace!("rsa receipt: {receipt:?}");

    risc0_zkp::verify::verify(
        &risc0_circuit_recursion::CIRCUIT,
        &hash_suite,
        &receipt.seal,
        |_, _| Ok(()),
    )
    .unwrap();
}

// Runs the end-to-end test using the rsa prover implementation
#[test]
fn prove_and_verify_rsa() {
    let [n, s, m] = golden_values().try_into().unwrap();
    let claim = crate::rsa::claim(&RSA_256_X2, n, s, m);

    let zkr = get_zkr("rsa_256_x2.zkr", BIGINT_PO2).unwrap();
    let receipt = prove::<sha::Impl>(&[&claim], &RSA_256_X2, zkr).unwrap();
    verify::<sha::Impl>(&crate::rsa::RSA_256_X2, &[&claim], &receipt).unwrap();
}

fn run_guest_compose(claims: &[impl Borrow<[BigUint; 3]>]) -> Result<()> {
    let claims: Vec<[BigUint; 3]> = claims.iter().map(Borrow::borrow).cloned().collect();
    let env = ExecutorEnv::builder()
        // Send a & b to the guest
        .write(&claims)?
        .build()?;

    crate::zkr::register_zkrs();

    // Obtain the default prover.
    let prover = default_prover();

    // Produce a receipt by proving the specified ELF binary.
    let receipt = prover.prove(env, RSA_ELF)?.receipt;

    // Make sure this receipt actually depends on the assumption;
    // otherwise this test might give a false negative.
    assert!(!receipt
        .inner
        .composite()
        .unwrap()
        .assumption_receipts
        .is_empty());

    // Make sure the receipt verifies OK
    receipt.verify(RSA_ID)?;

    Ok(())
}

// Tries a single claim
#[test]
fn guest_compose_oneclaim() {
    let vals: [BigUint; 3] = golden_values().try_into().unwrap();

    run_guest_compose(&[&vals]).unwrap()
}

// Completely fills up a zkr's claim-verifying capacity
#[test]
fn guest_compose_iters() {
    let vals: [BigUint; 3] = golden_values().try_into().unwrap();

    let claims = vec![&vals; RSA_256_X2.iters];
    run_guest_compose(&claims).unwrap()
}

// Exceeds a zkr's claim-verifying capacity; should not work at all.
#[test]
fn guest_compose_exceed_iters() {
    let vals: [BigUint; 3] = golden_values().try_into().unwrap();

    let claims = vec![&vals; RSA_256_X2.iters + 1];
    run_guest_compose(&claims).expect_err("Expected too many iterations error");
}

// Supplies no claims to the ZKR to verify; at least one is required.
#[test]
fn guest_compose_empty() {
    run_guest_compose(&[] as &[&[BigUint; 3]]).expect_err("Expected empty claims error");
}

// Makes sure composition fails if any of the data changes
#[test]
fn guest_compose_corrupted() {
    for idx in 0..3 {
        let mut vals: [BigUint; 3] = golden_values().try_into().unwrap();
        vals[idx] += 1usize;
        run_guest_compose(&[vals]).expect_err(&format!(
            "Expected zkr verification failure when corrupting RSA value #{idx}"
        ));
    }
}
