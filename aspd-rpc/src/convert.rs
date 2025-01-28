
use std::convert::TryFrom;
use std::fmt;

use bitcoin::secp256k1::schnorr;
use bitcoin::{self, FeeRate};

use ark::{musig, VtxoId};
use ark::tree::signed::VtxoTreeSpec;

#[derive(Debug)]
pub struct ConvertError {
	msg: &'static str,
}

impl From<&'static str> for ConvertError {
	fn from(msg: &'static str) -> ConvertError {
		ConvertError { msg }
	}
}

impl fmt::Display for ConvertError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "rpc conversion error: {}", self.msg)
	}
}

impl std::error::Error for ConvertError {}


impl From<ark::lightning::PaymentStatus> for crate::PaymentStatus {
	fn from(value: ark::lightning::PaymentStatus) -> Self {
		match value {
			ark::lightning::PaymentStatus::Complete => crate::PaymentStatus::Complete,
			ark::lightning::PaymentStatus::Pending => crate::PaymentStatus::Pending,
			ark::lightning::PaymentStatus::Failed => crate::PaymentStatus::Failed,
		}
	}
}

impl From<ark::rounds::RoundEvent> for crate::RoundEvent {
	fn from(e: ark::rounds::RoundEvent) -> Self {
		crate::RoundEvent {
			event: Some(match e {
				ark::rounds::RoundEvent::Start { round_id, offboard_feerate } => {
					crate::round_event::Event::Start(crate::RoundStart {
						round_id,
						offboard_feerate_sat_vkb: offboard_feerate.to_sat_per_kwu() * 4,
					})
				},
				ark::rounds::RoundEvent::Attempt { round_id, attempt } => {
					crate::round_event::Event::Attempt(crate::RoundAttempt {
						round_id, attempt,
					})
				},
				ark::rounds::RoundEvent::VtxoProposal {
					round_id, vtxos_spec, unsigned_round_tx, cosign_agg_nonces,
				} => {
					crate::round_event::Event::VtxoProposal(crate::VtxoProposal {
						round_id,
						vtxos_spec: vtxos_spec.encode(),
						unsigned_round_tx: bitcoin::consensus::serialize(&unsigned_round_tx),
						vtxos_agg_nonces: cosign_agg_nonces.into_iter()
							.map(|n| n.serialize().to_vec())
							.collect(),
					})
				},
				ark::rounds::RoundEvent::RoundProposal { round_id, cosign_sigs, forfeit_nonces } => {
					crate::round_event::Event::RoundProposal(crate::RoundProposal {
						round_id,
						vtxo_cosign_signatures: cosign_sigs.into_iter()
							.map(|s| s.serialize().to_vec()).collect(),
						forfeit_nonces: forfeit_nonces.into_iter().map(|(id, nonces)| {
							crate::ForfeitNonces {
								input_vtxo_id: id.to_bytes().to_vec(),
								pub_nonces: nonces.into_iter()
									.map(|n| n.serialize().to_vec())
									.collect(),
							}
						}).collect(),
					})
				},
				ark::rounds::RoundEvent::Finished { round_id, signed_round_tx } => {
					crate::round_event::Event::Finished(crate::RoundFinished {
						round_id,
						signed_round_tx: bitcoin::consensus::serialize(&signed_round_tx),
					})
				},
			})
		}
	}
}

impl TryFrom<crate::RoundEvent> for ark::rounds::RoundEvent {
	type Error = ConvertError;

	fn try_from(m: crate::RoundEvent) -> Result<ark::rounds::RoundEvent, Self::Error> {
		Ok(match m.event.unwrap() {
			crate::round_event::Event::Start(m) => {
				let offboard_feerate = FeeRate::from_sat_per_kwu(m.offboard_feerate_sat_vkb / 4);
				ark::rounds::RoundEvent::Start { round_id: m.round_id, offboard_feerate }
			},
			crate::round_event::Event::Attempt(m) => {
				ark::rounds::RoundEvent::Attempt { round_id: m.round_id, attempt: m.attempt }
			},
			crate::round_event::Event::VtxoProposal(m) => {
				ark::rounds::RoundEvent::VtxoProposal {
					round_id: m.round_id,
					unsigned_round_tx: bitcoin::consensus::deserialize(&m.unsigned_round_tx)
						.map_err(|_| "invalid unsigned_round_tx")?,
					vtxos_spec: VtxoTreeSpec::decode(&m.vtxos_spec)
						.map_err(|_| "invalid vtxos_spec")?,
					cosign_agg_nonces: m.vtxos_agg_nonces.into_iter().map(|n| {
						musig::MusigAggNonce::from_slice(&n)
							.map_err(|_| "invalid vtxos_agg_nonces")
					}).collect::<Result<_, _>>()?,
				}
			},
			crate::round_event::Event::RoundProposal(m) => {
				ark::rounds::RoundEvent::RoundProposal {
					round_id: m.round_id,
					cosign_sigs: m.vtxo_cosign_signatures.into_iter().map(|s| {
						schnorr::Signature::from_slice(&s)
							.map_err(|_| "invalid vtxo_cosign_signatures")
					}).collect::<Result<_, _>>()?,
					forfeit_nonces: m.forfeit_nonces.into_iter().map(|f| {
						let vtxo_id = VtxoId::from_slice(&f.input_vtxo_id)
							.map_err(|_| "invalid input_vtxo_id")?;
						let nonces = f.pub_nonces.into_iter().map(|n| {
							musig::MusigPubNonce::from_slice(&n)
								.map_err(|_| "invalid pub_nonces")
						}).collect::<Result<_, _>>()?;
						Ok((vtxo_id, nonces))
					}).collect::<Result<_, ConvertError>>()?,
				}
			},
			crate::round_event::Event::Finished(m) => {
				ark::rounds::RoundEvent::Finished {
					round_id: m.round_id,
					signed_round_tx: bitcoin::consensus::deserialize(&m.signed_round_tx)
						.map_err(|_| "invalid signed_round_tx")?,
				}
			},
		})
	}

}
