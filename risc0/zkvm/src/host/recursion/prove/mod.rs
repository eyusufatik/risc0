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

pub mod zkr;

use std::{collections::VecDeque, fmt::Debug};

use anyhow::{anyhow, ensure, Context, Result};
use risc0_circuit_recursion::{
    control_id::BN254_IDENTITY_CONTROL_ID,
    prove::{DigestKind, RecursionReceipt},
    CircuitImpl,
};
use risc0_circuit_rv32im::control_id::POSEIDON2_CONTROL_IDS;
use risc0_zkp::{
    adapter::{CircuitInfo, PROOF_SYSTEM_INFO},
    core::{digest::Digest, hash::hash_suite_from_name},
    field::baby_bear::{BabyBear, BabyBearElem, BabyBearExtElem},
    hal::{CircuitHal, Hal},
    verify::ReadIOP,
    MIN_CYCLES_PO2,
};
use serde::Serialize;

use crate::{
    receipt::{
        merkle::{MerkleGroup, MerkleProof},
        SegmentReceipt, SuccinctReceipt, SuccinctReceiptVerifierParameters,
    },
    receipt_claim::{Assumption, MaybePruned, Merge},
    sha::Digestible,
    ProverOpts, ReceiptClaim,
};

use risc0_circuit_recursion::prove::Program;

/// Number of rows to use for the recursion circuit witness as a power of 2.
pub const RECURSION_PO2: usize = 18;

/// Run the lift program to transform an rv32im segment receipt into a recursion receipt.
///
/// The lift program verifies the rv32im circuit STARK proof inside the recursion circuit,
/// resulting in a recursion circuit STARK proof. This recursion proof has a single
/// constant-time verification procedure, with respect to the original segment length, and is then
/// used as the input to all other recursion programs (e.g. join, resolve, and identity_p254).
pub fn lift(segment_receipt: &SegmentReceipt) -> Result<SuccinctReceipt<ReceiptClaim>> {
    tracing::debug!("Proving lift: claim = {:#?}", segment_receipt.claim);
    let opts = ProverOpts::succinct();
    let mut prover = Prover::new_lift(segment_receipt, opts.clone())?;

    let receipt = prover.prover.run()?;
    let mut out_stream = VecDeque::<u32>::new();
    out_stream.extend(receipt.output.iter());
    let claim_decoded = ReceiptClaim::decode(&mut out_stream)?;
    tracing::debug!("Proving lift finished: decoded claim = {claim_decoded:#?}");

    // Include an inclusion proof for control_id to allow verification against a root.
    let control_inclusion_proof = MerkleGroup::new(opts.control_ids.clone())?
        .get_proof(&prover.control_id, opts.hash_suite()?.hashfn.as_ref())?;
    Ok(SuccinctReceipt {
        seal: receipt.seal,
        hashfn: opts.hashfn,
        control_id: prover.control_id,
        control_inclusion_proof,
        claim: claim_decoded.merge(&segment_receipt.claim)?.into(),
        verifier_parameters: SuccinctReceiptVerifierParameters::default().digest(),
    })
}

/// Run the join program to compress two receipts of the same session into one.
///
/// By repeated application of the join program, any number of receipts for execution spans within
/// the same session can be compressed into a single receipt for the entire session.
pub fn join(
    a: &SuccinctReceipt<ReceiptClaim>,
    b: &SuccinctReceipt<ReceiptClaim>,
) -> Result<SuccinctReceipt<ReceiptClaim>> {
    tracing::debug!("Proving join: a.claim = {:#?}", a.claim);
    tracing::debug!("Proving join: b.claim = {:#?}", b.claim);

    let opts = ProverOpts::succinct();
    let mut prover = Prover::new_join(a, b, opts.clone())?;
    let receipt = prover.prover.run()?;
    let mut out_stream = VecDeque::<u32>::new();
    out_stream.extend(receipt.output.iter());

    // Construct the expected claim that should have result from the join.
    let ab_claim = ReceiptClaim {
        pre: a.claim.as_value()?.pre.clone(),
        post: b.claim.as_value()?.post.clone(),
        exit_code: b.claim.as_value()?.exit_code,
        input: a.claim.as_value()?.input.clone(),
        output: b.claim.as_value()?.output.clone(),
    };

    let claim_decoded = ReceiptClaim::decode(&mut out_stream)?;
    tracing::debug!("Proving join finished: decoded claim = {claim_decoded:#?}");

    // Include an inclusion proof for control_id to allow verification against a root.
    let control_inclusion_proof = MerkleGroup::new(opts.control_ids.clone())?
        .get_proof(&prover.control_id, opts.hash_suite()?.hashfn.as_ref())?;
    Ok(SuccinctReceipt {
        seal: receipt.seal,
        hashfn: opts.hashfn,
        control_id: prover.control_id,
        control_inclusion_proof,
        claim: claim_decoded.merge(&ab_claim)?.into(),
        verifier_parameters: SuccinctReceiptVerifierParameters::default().digest(),
    })
}

/// Run the resolve program to remove an assumption from a conditional receipt upon verifying a
/// receipt proving the validity of the assumption.
///
/// By applying the resolve program, a conditional receipt (i.e. a receipt for an execution using
/// the `env::verify` API to logically verify a receipt) can be made into an unconditional receipt.
pub fn resolve<Claim>(
    conditional: &SuccinctReceipt<ReceiptClaim>,
    assumption: &SuccinctReceipt<Claim>,
) -> Result<SuccinctReceipt<ReceiptClaim>>
where
    Claim: risc0_binfmt::Digestible + Debug + Clone + Serialize,
{
    tracing::debug!(
        "Proving resolve: conditional.claim = {:#?}",
        conditional.claim,
    );
    tracing::debug!(
        "Proving resolve: assumption.claim = {:#?}",
        assumption.claim,
    );

    // Construct the resolved claim by copying the conditional receipt claim and resolving
    // the head assumption. If this fails, then so would the resolve program.
    let mut resolved_claim = conditional
        .claim
        .as_value()
        .context("conditional receipt claim is pruned")?
        .clone();
    // Open the assumptions on the output of the claim and remove the first assumption.
    // NOTE: Prover::new_resolve will check that the assumption can actually be resolved with the
    // given receipts.
    resolved_claim
        .output
        .as_value_mut()
        .context("conditional receipt output is pruned")?
        .as_mut()
        .ok_or(anyhow!(
            "conditional receipt has empty output and no assumptions"
        ))?
        .assumptions
        .as_value_mut()
        .context("conditional receipt assumptions are pruned")?
        .0
        .drain(..1)
        .next()
        .ok_or(anyhow!(
            "cannot resolve assumption from receipt with no assumptions"
        ))?;

    let opts = ProverOpts::succinct();
    let mut prover = Prover::new_resolve(conditional, assumption, opts.clone())?;
    let receipt = prover.prover.run()?;
    let mut out_stream = VecDeque::<u32>::new();
    out_stream.extend(receipt.output.iter());

    let claim_decoded = ReceiptClaim::decode(&mut out_stream)?;
    tracing::debug!("Proving resolve finished: decoded claim = {claim_decoded:#?}");

    // Include an inclusion proof for control_id to allow verification against a root.
    let control_inclusion_proof = MerkleGroup::new(opts.control_ids.clone())?
        .get_proof(&prover.control_id, opts.hash_suite()?.hashfn.as_ref())?;
    Ok(SuccinctReceipt {
        seal: receipt.seal,
        hashfn: opts.hashfn,
        control_id: prover.control_id,
        control_inclusion_proof,
        claim: claim_decoded.merge(&resolved_claim)?.into(),
        verifier_parameters: SuccinctReceiptVerifierParameters::default().digest(),
    })
}

/// Prove the verification of a recursion receipt using the Poseidon254 hash function for FRI.
///
/// The identity_p254 program is used as the last step in the prover pipeline before running the
/// Groth16 prover. In Groth16 over BN254, it is much more efficient to verify a STARK that was
/// produced with Poseidon over the BN254 base field compared to using Poseidon over BabyBear.
pub fn identity_p254(a: &SuccinctReceipt<ReceiptClaim>) -> Result<SuccinctReceipt<ReceiptClaim>> {
    let opts = ProverOpts::succinct()
        .with_hashfn("poseidon_254".to_string())
        .with_control_ids(vec![BN254_IDENTITY_CONTROL_ID]);

    let mut prover = Prover::new_identity(a, opts.clone())?;
    let receipt = prover.prover.run()?;
    let mut out_stream = VecDeque::<u32>::new();
    out_stream.extend(receipt.output.iter());
    let claim = MaybePruned::Value(ReceiptClaim::decode(&mut out_stream)?).merge(&a.claim)?;

    // Include an inclusion proof for control_id to allow verification against a root.
    let hashfn = opts.hash_suite()?.hashfn;
    let control_inclusion_proof = MerkleGroup::new(opts.control_ids.clone())?
        .get_proof(&prover.control_id, hashfn.as_ref())?;
    let control_root = control_inclusion_proof.root(&prover.control_id, hashfn.as_ref());
    let params = SuccinctReceiptVerifierParameters {
        control_root,
        inner_control_root: Some(a.control_root()?),
        proof_system_info: PROOF_SYSTEM_INFO,
        circuit_info: CircuitImpl::CIRCUIT_INFO,
    };
    Ok(SuccinctReceipt {
        seal: receipt.seal,
        hashfn: opts.hashfn,
        control_id: prover.control_id,
        control_inclusion_proof,
        claim,
        verifier_parameters: params.digest(),
    })
}

/// Prove the test_recursion_circuit. This is useful for testing purposes.
///
/// digest1 will be passed through to the first of the output globals, as the "inner control root".
/// digest1 and digest2 will be used to calculate a "claim digest", placed in the second output.
#[cfg(test)]
pub fn test_recursion_circuit(
    digest1: &Digest,
    digest2: &Digest,
) -> Result<SuccinctReceipt<crate::receipt_claim::Unknown>> {
    let (_, control_id) = zkr::test_recursion_circuit("poseidon2")?;
    let opts = ProverOpts::succinct().with_control_ids(vec![control_id]);

    let mut prover = Prover::new_test_recursion_circuit([digest1, digest2], opts.clone())?;
    let receipt = prover.prover.run()?;

    // Read the claim digest from the second of the global output slots.
    const DIGEST_SHORTS: usize = crate::sha::DIGEST_WORDS * 2;
    let claim_digest = risc0_binfmt::read_sha_halfs(&mut VecDeque::from_iter(
        bytemuck::checked::cast_slice::<_, BabyBearElem>(
            &receipt.seal[DIGEST_SHORTS..2 * DIGEST_SHORTS],
        )
        .iter()
        .copied()
        .map(u32::from),
    ))?;

    // Include an inclusion proof for control_id to allow verification against a root.
    let hashfn = opts.hash_suite()?.hashfn;
    let control_inclusion_proof = MerkleGroup::new(opts.control_ids.clone())?
        .get_proof(&prover.control_id, hashfn.as_ref())?;
    let control_root = control_inclusion_proof.root(&prover.control_id, hashfn.as_ref());
    let params = SuccinctReceiptVerifierParameters {
        control_root,
        inner_control_root: Some(digest1.to_owned()),
        proof_system_info: PROOF_SYSTEM_INFO,
        circuit_info: CircuitImpl::CIRCUIT_INFO,
    };
    Ok(SuccinctReceipt {
        seal: receipt.seal,
        hashfn: opts.hashfn,
        control_id: prover.control_id,
        control_inclusion_proof,
        claim: MaybePruned::Pruned(claim_digest),
        verifier_parameters: params.digest(),
    })
}

/// Prover for zkVM use of the recursion circuit.
pub struct Prover {
    prover: risc0_circuit_recursion::prove::Prover,
    control_id: Digest,
}

impl Prover {
    fn new(program: Program, control_id: Digest, opts: ProverOpts) -> Self {
        Self {
            prover: risc0_circuit_recursion::prove::Prover::new(program, &opts.hashfn),
            control_id,
        }
    }

    /// Returns the control id of the recursion VM program being proven.
    pub fn control_id(&self) -> &Digest {
        &self.control_id
    }

    /// Initialize a recursion prover with the test recursion program. This program is used in
    /// testing the basic correctness of the recursion circuit.
    pub fn new_test_recursion_circuit(digests: [&Digest; 2], opts: ProverOpts) -> Result<Self> {
        let (program, control_id) = zkr::test_recursion_circuit(&opts.hashfn)?;
        let mut prover = Prover::new(program, control_id, opts);

        for digest in digests {
            prover.add_input_digest(digest, DigestKind::Poseidon2);
        }

        Ok(prover)
    }

    /// Initialize a recursion prover with the lift program to transform an rv32im segment receipt
    /// into a recursion receipt.
    ///
    /// The lift program is verifies the rv32im circuit STARK proof inside the recursion circuit,
    /// resulting in a recursion circuit STARK proof. This recursion proof has a single
    /// constant-time verification procedure, with respect to the original segment length, and is
    /// then used as the input to all other recursion programs (e.g. join, resolve, and
    /// identity_p254).
    pub fn new_lift(segment: &SegmentReceipt, opts: ProverOpts) -> Result<Self> {
        ensure!(
            segment.hashfn == "poseidon2",
            "lift recursion program only supports poseidon2 hashfn; received {}",
            segment.hashfn
        );

        let inner_hash_suite = hash_suite_from_name(&segment.hashfn)
            .ok_or_else(|| anyhow!("unsupported hash function: {}", segment.hashfn))?;
        let allowed_ids = MerkleGroup::new(opts.control_ids.clone())?;
        let merkle_root = allowed_ids.calc_root(inner_hash_suite.hashfn.as_ref());

        // Read the output fields in the rv32im seal to get the po2. We need this po2 to chose
        // which lift program we are going to run.
        let mut iop = ReadIOP::new(&segment.seal, inner_hash_suite.rng.as_ref());
        iop.read_field_elem_slice::<BabyBearElem>(risc0_circuit_rv32im::CircuitImpl::OUTPUT_SIZE);
        let po2 = *iop.read_u32s(1).first().unwrap() as usize;

        // Instantiate the prover with the lift recursion program and its control ID.
        let (program, control_id) = zkr::lift(po2, &opts.hashfn)?;
        let mut prover = Prover::new(program, control_id, opts);

        prover.add_input_digest(&merkle_root, DigestKind::Poseidon2);

        // Get the control ID for the rv32im with the given po2. It must also be in the allowed IDs.
        let which = po2 - MIN_CYCLES_PO2;
        let inner_control_id = POSEIDON2_CONTROL_IDS[which];
        prover.add_seal(
            &segment.seal,
            &inner_control_id,
            &allowed_ids.get_proof(&inner_control_id, inner_hash_suite.hashfn.as_ref())?,
        )?;

        Ok(prover)
    }

    /// Initialize a recursion prover with the join program to compress two receipts of the same
    /// session into one.
    ///
    /// By repeated application of the join program, any number of receipts for execution spans
    /// within the same session can be compressed into a single receipt for the entire session.
    pub fn new_join(
        a: &SuccinctReceipt<ReceiptClaim>,
        b: &SuccinctReceipt<ReceiptClaim>,
        opts: ProverOpts,
    ) -> Result<Self> {
        ensure!(
            a.hashfn == "poseidon2",
            "join recursion program only supports poseidon2 hashfn; received {}",
            a.hashfn
        );
        ensure!(
            b.hashfn == "poseidon2",
            "join recursion program only supports poseidon2 hashfn; received {}",
            b.hashfn
        );

        let (program, control_id) = zkr::join(&opts.hashfn)?;
        let mut prover = Prover::new(program, control_id, opts);

        // Determine the control root from the receipts themselves, and ensure they are equal. If
        // the determined control root does not match what the downstream verifier expects, they
        // will reject.
        let merkle_root = a.control_root()?;
        ensure!(
            merkle_root == b.control_root()?,
            "merkle roots for a and b do not match: {} != {}",
            merkle_root,
            b.control_root()?
        );

        prover.add_input_digest(&merkle_root, DigestKind::Poseidon2);
        prover.add_segment_receipt(a)?;
        prover.add_segment_receipt(b)?;
        Ok(prover)
    }

    /// Initialize a recursion prover with the resolve program to remove an assumption from a
    /// conditional receipt upon verifying a receipt proving the validity of the assumption.
    ///
    /// By applying the resolve program, a conditional receipt (i.e. a receipt for an execution
    /// using the `env::verify` API to logically verify a receipt) can be made into an
    /// unconditional receipt.
    pub fn new_resolve<Claim>(
        cond: &SuccinctReceipt<ReceiptClaim>,
        assum: &SuccinctReceipt<Claim>,
        opts: ProverOpts,
    ) -> Result<Self>
    where
        Claim: risc0_binfmt::Digestible + Debug + Clone + Serialize,
    {
        ensure!(
            cond.hashfn == "poseidon2",
            "resolve recursion program only supports poseidon2 hashfn; received {}",
            cond.hashfn
        );
        ensure!(
            assum.hashfn == "poseidon2",
            "resolve recursion program only supports poseidon2 hashfn; received {}",
            assum.hashfn
        );

        // Load the resolve predicate as a Program and construct the prover.
        let (program, control_id) = zkr::resolve(&opts.hashfn)?;
        let mut prover = Prover::new(program, control_id, opts);

        // Load the input values needed by the predicate.
        // Resolve predicate needs both seals as input, and the journal and assumptions tail digest
        // to compute the opening of the conditional receipt claim to the first assumption.
        prover.add_input_digest(&cond.control_root()?, DigestKind::Poseidon2);
        prover.add_segment_receipt(cond)?;

        let output = cond
            .claim
            .as_value()
            .context("cannot resolve conditional receipt with pruned claim")?
            .output
            .as_value()
            .context("cannot resolve conditional receipt with pruned output")?
            .as_ref()
            .ok_or(anyhow!("cannot resolve conditional receipt with no output"))?
            .clone();

        // Unwrap the MaybePruned assumptions list and resolve the corroborated assumption,
        // removing the head and leaving the tail of the list.
        let assumptions = output
            .assumptions
            .value()
            .context("cannot resolve conditional receipt with pruned assumptions")?;
        let head: Assumption = assumptions
            .0
            .first()
            .ok_or(anyhow!(
                "cannot resolve conditional receipt with no assumptions"
            ))?
            .as_value()
            .context("cannot resolve conditional receipt with pruned head assumption")?
            .clone();

        // Ensure that the assumption receipt can resolve the assumption.
        ensure!(
            head.claim == assum.claim.digest(),
            "assumption receipt claim does not match head of assumptions list"
        );
        let expected_root = match head.control_root == Digest::ZERO {
            true => cond.control_root()?,
            false => head.control_root,
        };
        ensure!(
            expected_root == assum.control_root()?,
            "assumption receipt control root does not match head of assumptions list"
        );

        let mut assumptions_tail = assumptions;
        assumptions_tail.resolve(&head.digest())?;

        prover.add_assumption_receipt(head, assum)?;
        prover.add_input_digest(&assumptions_tail.digest(), DigestKind::Sha256);
        prover.add_input_digest(&output.journal.digest(), DigestKind::Sha256);
        Ok(prover)
    }

    /// Prove the verification of a recursion receipt, applying no changes to [ReceiptClaim].
    ///
    /// The primary use for this program is to transform the receipt itself, e.g. using a different
    /// hash function for FRI. See [identity_p254] for more information.
    pub fn new_identity(a: &SuccinctReceipt<ReceiptClaim>, opts: ProverOpts) -> Result<Self> {
        ensure!(
            a.hashfn == "poseidon2",
            "identity recursion program only supports poseidon2 hashfn; received {}",
            a.hashfn
        );

        let (program, control_id) = zkr::identity(&opts.hashfn)?;
        let mut prover = Prover::new(program, control_id, opts);

        prover.add_input_digest(&a.control_root()?, DigestKind::Poseidon2);
        prover.add_segment_receipt(a)?;
        Ok(prover)
    }

    fn add_input(&mut self, input: &[u32]) {
        self.prover.add_input(input)
    }

    /// Add a digest to the input for the recursion program.
    fn add_input_digest(&mut self, digest: &Digest, kind: DigestKind) {
        self.prover.add_input_digest(digest, kind)
    }

    /// Add a recursion seal (i.e. STARK proof) to input tape of the recursion program.
    pub fn add_seal(
        &mut self,
        seal: &[u32],
        control_id: &Digest,
        control_inclusion_proof: &MerkleProof,
    ) -> Result<()> {
        tracing::debug!("Control ID = {:?}", control_id);
        self.add_input(seal);
        tracing::debug!("index = {:?}", control_inclusion_proof.index);
        self.add_input(bytemuck::cast_slice(&[BabyBearElem::new(
            control_inclusion_proof.index,
        )]));
        for digest in &control_inclusion_proof.digests {
            tracing::debug!("path = {:?}", digest);
            self.add_input_digest(digest, DigestKind::Poseidon2);
        }
        Ok(())
    }

    /// Add a receipt covering some generic claim. Do not include any claim information.
    fn add_assumption_receipt<Claim>(
        &mut self,
        assumption: Assumption,
        receipt: &SuccinctReceipt<Claim>,
    ) -> Result<()>
    where
        Claim: risc0_binfmt::Digestible + Debug + Clone + Serialize,
    {
        self.add_seal(
            &receipt.seal,
            &receipt.control_id,
            &receipt.control_inclusion_proof,
        )?;
        // Resolve program expects an additional boolean to tell it when the control root is zero.
        let zero_root = BabyBearElem::new((assumption.control_root == Digest::ZERO) as u32);
        self.add_input(bytemuck::cast_slice(&[zero_root]));
        Ok(())
    }

    /// Add a receipt covering rv32im execution, and include the first level of ReceiptClaim.
    fn add_segment_receipt(&mut self, a: &SuccinctReceipt<ReceiptClaim>) -> Result<()> {
        self.add_seal(&a.seal, &a.control_id, &a.control_inclusion_proof)?;
        let mut data = Vec::<u32>::new();
        a.claim.as_value()?.encode(&mut data)?;
        let data_fp: Vec<BabyBearElem> = data.iter().map(|x| BabyBearElem::new(*x)).collect();
        self.add_input(bytemuck::cast_slice(&data_fp));
        Ok(())
    }

    /// Run the prover, producing a receipt of execution for the recursion circuit over the loaded
    /// program and input.
    #[tracing::instrument(skip_all)]
    pub fn run(&mut self) -> Result<RecursionReceipt> {
        self.prover.run()
    }

    /// Run the prover, producing a receipt of execution for the recursion circuit over the loaded
    /// program and input, using the specified HAL.
    #[tracing::instrument(skip_all)]
    pub fn run_with_hal<H, C>(&mut self, hal: &H, circuit_hal: &C) -> Result<RecursionReceipt>
    where
        H: Hal<Field = BabyBear, Elem = BabyBearElem, ExtElem = BabyBearExtElem>,
        C: CircuitHal<H>,
    {
        self.prover.run_with_hal(hal, circuit_hal)
    }
}
