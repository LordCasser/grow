use super::tool_history::{DomainReference, read_domain};
use chat_state::{
    EventSeq, InputAdmissionDecision, InputCheckpointEntry, InputDismissReason, InputEvent,
    InputLifecycle, InputState, TimelineEvent, TimelineEventKind, TurnEvent, TurnId,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs::File, io};

const SUBMISSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("input_submission");
const PAYLOADS: TableDefinition<&str, &[u8]> = TableDefinition::new("input_payload");
const STATES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("input_state");
const LIVE: TableDefinition<&str, &[u8]> = TableDefinition::new("input_live");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("input_at_frontier");

#[derive(Serialize, Deserialize)]
struct Version {
    state: InputState,
    source: DomainReference,
}

#[derive(Serialize, Deserialize)]
struct Live {
    position: u64,
    reserved_turn: Option<TurnId>,
}

pub(super) struct Writer<'tx> {
    payloads: Table<'tx, &'static str, &'static [u8]>,
    submissions: Table<'tx, &'static str, &'static [u8]>,
    states: Table<'tx, (&'static str, u64), &'static [u8]>,
    live: Table<'tx, &'static str, &'static [u8]>,
    cuts: Table<'tx, u64, &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

fn version_before(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    id: &str,
    before: EventSeq,
) -> io::Result<Option<Version>> {
    table
        .range((id, 0)..(id, before.get()))
        .map_err(io::Error::other)?
        .next_back()
        .transpose()
        .map_err(io::Error::other)?
        .map(|(_, value)| decode(value.value()))
        .transpose()
}

impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            payloads: tx.open_table(PAYLOADS).map_err(io::Error::other)?,
            submissions: tx.open_table(SUBMISSIONS).map_err(io::Error::other)?,
            states: tx.open_table(STATES).map_err(io::Error::other)?,
            live: tx.open_table(LIVE).map_err(io::Error::other)?,
            cuts: tx.open_table(CUTS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        kind: &TimelineEventKind,
        source: DomainReference,
    ) -> io::Result<()> {
        let event = match kind {
            TimelineEventKind::Input(event) => event,
            TimelineEventKind::Turn(TurnEvent::Started { id, input_ids, .. }) => {
                for input in input_ids {
                    let mut live = self.read_live(input)?;
                    live.reserved_turn = Some(*id);
                    self.write_live(input, live)?;
                }
                return Ok(());
            }
            TimelineEventKind::Turn(TurnEvent::Ended { id, .. }) => {
                let mut released = Vec::new();
                for row in self.live.iter().map_err(io::Error::other)? {
                    let (key, value) = row.map_err(io::Error::other)?;
                    let mut live: Live = decode(value.value())?;
                    if live.reserved_turn == Some(*id) {
                        live.reserved_turn = None;
                        released.push((key.value().to_owned(), live));
                    }
                }
                for (input, live) in released {
                    self.write_live(&input, live)?;
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        match event {
            InputEvent::Submitted {
                input_id,
                payload_ref,
                ..
            } => {
                let encoded = serde_json::to_vec(&source)?;
                if self
                    .submissions
                    .insert(input_id.as_str(), encoded.as_slice())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Input submission"));
                }
                if self
                    .payloads
                    .get(payload_ref.blake3.as_str())
                    .map_err(io::Error::other)?
                    .is_none()
                {
                    self.payloads
                        .insert(payload_ref.blake3.as_str(), encoded.as_slice())
                        .map_err(io::Error::other)?;
                }
                self.write_state(input_id, InputState::Submitted, source)?;
                self.write_live(
                    input_id,
                    Live {
                        position: source.seq.get(),
                        reserved_turn: None,
                    },
                )?;
            }
            InputEvent::AdmissionResolved {
                input_id,
                decision,
                route,
                supersedes,
            } => {
                let mut live = self.read_live(input_id)?;
                for superseded in supersedes {
                    live.position = live.position.min(self.read_live(superseded)?.position);
                    self.terminal(
                        superseded,
                        InputState::Dismissed {
                            reason: InputDismissReason::RouteSuperseded,
                        },
                        source,
                    )?;
                }
                match (decision, route) {
                    (InputAdmissionDecision::Allow, Some(route)) => {
                        self.write_state(
                            input_id,
                            InputState::Allowed {
                                route: route.clone(),
                            },
                            source,
                        )?;
                        self.write_live(input_id, live)?;
                    }
                    (InputAdmissionDecision::Block { reason }, None) => {
                        self.terminal(
                            input_id,
                            InputState::Blocked {
                                reason: reason.clone(),
                            },
                            source,
                        )?;
                    }
                    _ => return Err(invalid("invalid indexed Input admission")),
                }
            }
            InputEvent::Rerouted { input_ids, route } => {
                for id in input_ids {
                    self.write_state(
                        id,
                        InputState::Allowed {
                            route: route.clone(),
                        },
                        source,
                    )?;
                }
            }
            InputEvent::Consumed { input_ids, .. } | InputEvent::Handled { input_ids, .. } => {
                for id in input_ids {
                    let prior = version_before(&self.states, id, source.seq)?;
                    let Some(Version {
                        state: InputState::Allowed { route },
                        ..
                    }) = prior
                    else {
                        return Err(invalid("indexed Input has no admitted route"));
                    };
                    let state = if matches!(event, InputEvent::Consumed { .. }) {
                        InputState::Consumed { route }
                    } else {
                        InputState::Handled { route }
                    };
                    self.terminal(id, state, source)?;
                }
            }
            InputEvent::Dismissed { input_ids, reason } => {
                for id in input_ids {
                    self.terminal(id, InputState::Dismissed { reason: *reason }, source)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn contains_live(&self, id: &str) -> io::Result<bool> {
        Ok(self.live.get(id).map_err(io::Error::other)?.is_some())
    }

    fn read_live(&self, id: &str) -> io::Result<Live> {
        let value = self
            .live
            .get(id)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("indexed Input has no live submission"))?;
        decode(value.value())
    }

    fn write_live(&mut self, id: &str, live: Live) -> io::Result<()> {
        self.live
            .insert(id, serde_json::to_vec(&live)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn write_state(
        &mut self,
        id: &str,
        state: InputState,
        source: DomainReference,
    ) -> io::Result<()> {
        self.states
            .insert(
                (id, source.seq.get()),
                serde_json::to_vec(&Version { state, source })?.as_slice(),
            )
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn terminal(&mut self, id: &str, state: InputState, source: DomainReference) -> io::Result<()> {
        self.write_state(id, state, source)?;
        self.live
            .remove(id)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("indexed Input terminal has no live submission"))?;
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        let mut inputs = Vec::new();
        for row in self.live.iter().map_err(io::Error::other)? {
            let (id, value) = row.map_err(io::Error::other)?;
            let live: Live = decode(value.value())?;
            inputs.push((id.value().to_owned(), live));
        }
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&inputs)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn open_inputs(
    tx: &ReadTransaction,
    journal: &mut File,
    bytes: u64,
    before: EventSeq,
) -> io::Result<Vec<InputCheckpointEntry>> {
    let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
    let value = cuts
        .get(bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing published Input checkpoint"))?;
    let inputs: Vec<(String, Live)> = decode(value.value())?;
    inputs
        .into_iter()
        .map(|(id, live)| {
            let (_, lifecycle) = input_before(tx, journal, &id, bytes, before)?
                .ok_or_else(|| invalid("published live Input has no lifecycle"))?;
            Ok(InputCheckpointEntry {
                lifecycle,
                canonical_position: live.position,
                reserved_turn: live.reserved_turn,
            })
        })
        .collect()
}

pub(super) fn input_before(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    bytes: u64,
    before: EventSeq,
) -> io::Result<Option<(TimelineEvent, InputLifecycle)>> {
    let submissions = tx.open_table(SUBMISSIONS).map_err(io::Error::other)?;
    let Some(value) = submissions.get(id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let reference: DomainReference = decode(value.value())?;
    if reference.seq >= before {
        return Ok(None);
    }
    let submitted = read_domain(journal, reference, bytes)?;
    let TimelineEventKind::Input(InputEvent::Submitted {
        input_id,
        intent,
        payload_ref,
    }) = &submitted.kind
    else {
        return Err(invalid("indexed Input has no canonical submission"));
    };
    if input_id != id {
        return Err(invalid(
            "indexed Input identity differs from its submission",
        ));
    }
    let states = tx.open_table(STATES).map_err(io::Error::other)?;
    let version = version_before(&states, id, before)?
        .ok_or_else(|| invalid("indexed Input has no state at its cut"))?;
    let source = if reference.seq == version.source.seq {
        submitted.clone()
    } else {
        read_domain(journal, version.source, bytes)?
    };
    if !valid_state_source(id, &version.state, &source.kind) {
        return Err(invalid(
            "indexed Input state differs from its canonical source",
        ));
    }
    if let InputState::Consumed { route } | InputState::Handled { route } = &version.state {
        let prior = version_before(&states, id, version.source.seq)?
            .ok_or_else(|| invalid("indexed Input terminal has no route source"))?;
        let prior_source = read_domain(journal, prior.source, bytes)?;
        if !matches!(&prior.state, InputState::Allowed { route: stored } if stored == route)
            || !valid_state_source(id, &prior.state, &prior_source.kind)
        {
            return Err(invalid(
                "indexed Input terminal differs from its admitted route",
            ));
        }
    }
    let lifecycle = InputLifecycle {
        input_id: input_id.clone(),
        intent: *intent,
        payload_ref: payload_ref.clone(),
        state: version.state,
    };
    Ok(Some((submitted, lifecycle)))
}

fn valid_state_source(id: &str, state: &InputState, kind: &TimelineEventKind) -> bool {
    let TimelineEventKind::Input(event) = kind else {
        return false;
    };
    match (state, event) {
        (InputState::Submitted, InputEvent::Submitted { input_id, .. }) => input_id == id,
        (
            InputState::Allowed { route },
            InputEvent::AdmissionResolved {
                input_id,
                decision: InputAdmissionDecision::Allow,
                route: Some(source),
                ..
            },
        ) => input_id == id && route == source,
        (
            InputState::Allowed { route },
            InputEvent::Rerouted {
                input_ids,
                route: source,
            },
        ) => route == source && input_ids.iter().any(|input| input == id),
        (
            InputState::Blocked { reason },
            InputEvent::AdmissionResolved {
                input_id,
                decision: InputAdmissionDecision::Block { reason: source },
                ..
            },
        ) => input_id == id && reason == source,
        (
            InputState::Dismissed {
                reason: InputDismissReason::RouteSuperseded,
            },
            InputEvent::AdmissionResolved { supersedes, .. },
        ) => supersedes.iter().any(|input| input == id),
        (
            InputState::Dismissed { reason },
            InputEvent::Dismissed {
                input_ids,
                reason: source,
            },
        ) => reason == source && input_ids.iter().any(|input| input == id),
        (InputState::Consumed { .. }, InputEvent::Consumed { input_ids, .. })
        | (InputState::Handled { .. }, InputEvent::Handled { input_ids, .. }) => {
            input_ids.iter().any(|input| input == id)
        }
        _ => false,
    }
}

pub(super) fn payload_hashes(
    tx: &ReadTransaction,
    journal: &mut File,
    bytes: u64,
    before: EventSeq,
    candidates: Option<&[String]>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<BTreeSet<String>> {
    let payloads = tx.open_table(PAYLOADS).map_err(io::Error::other)?;
    let mut hashes = BTreeSet::new();
    let mut inspect = |hash: &str, value: &[u8]| -> io::Result<()> {
        if cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "input payload inspection cancelled",
            ));
        }
        let reference: DomainReference = decode(value)?;
        if reference.seq < before {
            let event = read_domain(journal, reference, bytes)?;
            if !matches!(event.kind, TimelineEventKind::Input(InputEvent::Submitted { payload_ref, .. })
                if payload_ref.blake3 == hash)
            {
                return Err(invalid(
                    "Input payload retention root differs from its identity",
                ));
            }
            hashes.insert(hash.to_owned());
        }
        Ok(())
    };
    if let Some(candidates) = candidates {
        for hash in candidates {
            if let Some(value) = payloads.get(hash.as_str()).map_err(io::Error::other)? {
                inspect(hash, value.value())?;
            }
        }
    } else {
        for row in payloads.iter().map_err(io::Error::other)? {
            let (hash, value) = row.map_err(io::Error::other)?;
            inspect(hash.value(), value.value())?;
        }
    }
    Ok(hashes)
}
