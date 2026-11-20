//! Checks that the SPEL v2 IDL (`idl/vesting.json`) matches the vesting program byte for byte.
//!
//! For every message, the state and every event, a value is encoded twice: once by walking the
//! IDL's types (an encoder written here from the Borsh rules, independent of the program's Rust
//! types), once by Borsh-serialising the program's own types (`vesting-program`, `vesting-core`).
//! Any drift between the IDL and the program fails a byte comparison. Everything is offline.

#[cfg(test)]
mod tests {
    use lee_core::account::AccountId;
    use serde_json::{json, Value};
    use sha2::Digest;
    use vesting_core::{CancelRecord, Event, Schedule, ScheduleKind};
    use vesting_program::{
        Asset, Instruction, NewSchedule, Op, Pending, ScheduleState, Sponsor, VestingEvent,
    };

    fn idl() -> Value {
        serde_json::from_str(include_str!("../../idl/vesting.json")).expect("valid IDL JSON")
    }

    fn type_by_name<'a>(idl: &'a Value, name: &str) -> &'a Value {
        idl["types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("IDL has no type {name}"))
    }

    /// Borsh encoding of `value` as IDL type `ty`. Enum values are `{"variant": name, ..fields}`;
    /// u128 values are decimal strings; account ids and byte arrays are hex.
    fn encode(idl: &Value, ty: &Value, value: &Value) -> Vec<u8> {
        if let Some(name) = ty.as_str() {
            return match name {
                "u8" => vec![u8::try_from(value.as_u64().expect("u8")).unwrap()],
                "u16" => u16::try_from(value.as_u64().expect("u16"))
                    .unwrap()
                    .to_le_bytes()
                    .to_vec(),
                "u32" => u32::try_from(value.as_u64().expect("u32"))
                    .unwrap()
                    .to_le_bytes()
                    .to_vec(),
                "u64" => value.as_u64().expect("u64").to_le_bytes().to_vec(),
                "u128" => value
                    .as_str()
                    .expect("u128 as a decimal string")
                    .parse::<u128>()
                    .unwrap()
                    .to_le_bytes()
                    .to_vec(),
                "bool" => vec![u8::from(value.as_bool().expect("bool"))],
                "account_id" => {
                    let bytes = hex::decode(value.as_str().expect("hex account")).unwrap();
                    assert_eq!(bytes.len(), 32, "an account id is 32 bytes");
                    bytes
                }
                other => panic!("unhandled primitive IDL type {other}"),
            };
        }
        let object = ty.as_object().expect("an object type");
        if let Some(inner) = object.get("vec") {
            let items = value.as_array().expect("array value");
            let mut out = u32::try_from(items.len()).unwrap().to_le_bytes().to_vec();
            for item in items {
                out.extend(encode(idl, inner, item));
            }
            return out;
        }
        if let Some(inner) = object.get("option") {
            return match value {
                Value::Null => vec![0],
                some => [vec![1], encode(idl, inner, some)].concat(),
            };
        }
        if let Some(inner) = object.get("array") {
            let count = usize::try_from(inner[1].as_u64().unwrap()).unwrap();
            assert_eq!(inner[0], "u8", "only byte arrays are used");
            let bytes = hex::decode(value.as_str().expect("hex bytes")).unwrap();
            assert_eq!(bytes.len(), count, "fixed array length");
            return bytes;
        }
        let name = object
            .get("defined")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("unhandled IDL type {ty}"));
        let def = type_by_name(idl, name);
        if def["kind"] == "enum" {
            let variant = value["variant"]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: enum value needs 'variant'"));
            let variants = def["variants"].as_array().unwrap();
            let index = variants
                .iter()
                .position(|entry| entry["name"] == variant)
                .unwrap_or_else(|| panic!("type {name} has no variant {variant}"));
            let mut out = vec![u8::try_from(index).unwrap()];
            for field in variants[index]["fields"].as_array().unwrap() {
                out.extend(encode(
                    idl,
                    &field["type"],
                    &value[field["name"].as_str().unwrap()],
                ));
            }
            return out;
        }
        assert_eq!(def["kind"], "struct", "{name}");
        let mut out = Vec::new();
        for field in def["fields"].as_array().unwrap() {
            let field_name = field["name"].as_str().unwrap();
            assert!(
                value.get(field_name).is_some(),
                "{name}: the test value lacks field {field_name}"
            );
            out.extend(encode(idl, &field["type"], &value[field_name]));
        }
        out
    }

    fn message<'a>(idl: &'a Value, name: &str) -> &'a Value {
        idl["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name)
            .unwrap_or_else(|| panic!("no message {name}"))
    }

    fn encode_message(idl: &Value, name: &str, values: &Value) -> Vec<u8> {
        let m = message(idl, name);
        let mut out = vec![u8::try_from(m["tag"].as_u64().unwrap()).unwrap()];
        for field in m["fields"].as_array().unwrap() {
            out.extend(encode(
                idl,
                &field["type"],
                &values[field["name"].as_str().unwrap()],
            ));
        }
        out
    }

    fn a(byte: u8) -> AccountId {
        AccountId::new([byte; 32])
    }
    fn h(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn new_schedule() -> (NewSchedule, Value) {
        (
            NewSchedule {
                nonce: [3; 32],
                beneficiary: a(4),
                asset: Asset::Token {
                    program: a(5),
                    definition: a(6),
                },
                total: 1_000,
                kind: ScheduleKind::Milestone {
                    tranches: vec![400, 600],
                },
                cancelable: Some(true),
                transferable: None,
                cancel_authority: Some(a(7)),
                milestone_authority: None,
                sponsor: Some(Sponsor {
                    deposit: 50,
                    per_claim: 10,
                }),
            },
            json!({
                "nonce": h(3), "beneficiary": h(4),
                "asset": { "variant": "Token", "program": h(5), "definition": h(6) },
                "total": "1000",
                "kind": { "variant": "Milestone", "tranches": ["400", "600"] },
                "cancelable": true, "transferable": null, "cancel_authority": h(7),
                "milestone_authority": null, "sponsor": { "deposit": "50", "per_claim": "10" },
            }),
        )
    }

    #[test]
    fn the_idl_is_spel_v2_with_all_seven_messages_in_tag_order() {
        let idl = idl();
        assert_eq!(idl["version"], "2");
        assert_eq!(idl["state"], json!({ "defined": "ScheduleState" }));
        let names: Vec<_> = idl["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Create",
                "Claim",
                "ClaimAsOf",
                "Cancel",
                "MakeIrrevocable",
                "SignalMilestone",
                "TransferBeneficiary"
            ]
        );
        for (index, m) in idl["messages"].as_array().unwrap().iter().enumerate() {
            assert_eq!(m["tag"].as_u64().unwrap(), index as u64);
        }
    }

    #[test]
    fn every_message_encodes_like_the_program() {
        let idl = idl();
        let (n, nv) = new_schedule();
        let (s, t) = (a(1), a(2));
        let cases: Vec<(&str, Instruction, Value)> = vec![
            (
                "Create",
                Instruction::Create { schedules: vec![n] },
                json!({ "schedules": [nv] }),
            ),
            (
                "Claim",
                Instruction::Claim {
                    schedule: s,
                    to: t,
                    to_private: true,
                    granularity: 60_000,
                },
                json!({ "schedule": h(1), "to": h(2), "to_private": true, "granularity": 60_000 }),
            ),
            (
                "ClaimAsOf",
                Instruction::ClaimAsOf {
                    schedule: s,
                    to: t,
                    to_private: false,
                    as_of: 1_795_000_000_000,
                },
                json!({ "schedule": h(1), "to": h(2), "to_private": false, "as_of": 1_795_000_000_000u64 }),
            ),
            (
                "Cancel",
                Instruction::Cancel { schedule: s },
                json!({ "schedule": h(1) }),
            ),
            (
                "MakeIrrevocable",
                Instruction::MakeIrrevocable { schedule: s },
                json!({ "schedule": h(1) }),
            ),
            (
                "SignalMilestone",
                Instruction::SignalMilestone {
                    schedule: s,
                    index: 7,
                },
                json!({ "schedule": h(1), "index": 7 }),
            ),
            (
                "TransferBeneficiary",
                Instruction::TransferBeneficiary {
                    schedule: s,
                    new_beneficiary: t,
                },
                json!({ "schedule": h(1), "new_beneficiary": h(2) }),
            ),
        ];
        for (name, real, values) in cases {
            assert_eq!(
                encode_message(&idl, name, &values),
                borsh::to_vec(&real).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn the_state_encodes_like_the_program() {
        let idl = idl();
        let (n, nv) = new_schedule();
        let schedule = Schedule {
            creator: [1; 32],
            beneficiary: [4; 32],
            token: [6; 32],
            total: 1_000,
            claimed: 400,
            kind: ScheduleKind::CliffLinear {
                start: 10,
                cliff: 20,
                end: 30,
            },
            cancelable: true,
            transferable: false,
            cancel_authority: [7; 32],
            milestone_authority: [1; 32],
            signalled: vec![1],
            cancelled: Some(CancelRecord {
                at: 25,
                vested_at_cancel: 750,
            }),
            last_time: 25,
            version: 1,
        };
        let schedule_v = json!({
            "creator": h(1), "beneficiary": h(4), "token": h(6), "total": "1000", "claimed": "400",
            "kind": { "variant": "CliffLinear", "start": 10, "cliff": 20, "end": 30 },
            "cancelable": true, "transferable": false, "cancel_authority": h(7), "milestone_authority": h(1),
            "signalled": [1], "cancelled": { "at": 25, "vested_at_cancel": "750" }, "last_time": 25, "version": 1,
        });
        let cases = [
            (
                ScheduleState {
                    version: 1,
                    nonce: [3; 32],
                    asset: Asset::Native,
                    schedule: Some(schedule),
                    float: 0,
                    per_claim: 0,
                    pending: Some(Pending::Op {
                        caller: a(4),
                        op: Op::Claim {
                            to: a(9),
                            to_private: true,
                        },
                    }),
                },
                json!({
                    "version": 1, "nonce": h(3), "asset": { "variant": "Native" }, "schedule": schedule_v,
                    "float": "0", "per_claim": "0",
                    "pending": { "variant": "Op", "caller": h(4), "op": { "variant": "Claim", "to": h(9), "to_private": true } },
                }),
            ),
            (
                // Inside the creating transaction: no schedule yet, the creation pending.
                ScheduleState {
                    version: 1,
                    nonce: [3; 32],
                    asset: Asset::Token {
                        program: a(5),
                        definition: a(6),
                    },
                    schedule: None,
                    float: 50,
                    per_claim: 10,
                    pending: Some(Pending::Create {
                        creator: a(1),
                        params: n,
                    }),
                },
                json!({
                    "version": 1, "nonce": h(3), "asset": { "variant": "Token", "program": h(5), "definition": h(6) },
                    "schedule": null, "float": "50", "per_claim": "10",
                    "pending": { "variant": "Create", "creator": h(1), "params": nv },
                }),
            ),
        ];
        for (real, value) in cases {
            assert_eq!(
                encode(&idl, &idl["state"], &value),
                borsh::to_vec(&real).unwrap()
            );
        }
    }

    #[test]
    fn every_event_encodes_like_the_program_with_its_selector() {
        let idl = idl();
        let kind = ScheduleKind::Linear { start: 1, end: 2 };
        let cases: Vec<(&str, Event, Value)> = vec![
            (
                "Created",
                Event::Created {
                    creator: [1; 32],
                    beneficiary: [2; 32],
                    token: [0; 32],
                    total: 9,
                    kind,
                    cancelable: true,
                    transferable: false,
                    cancel_authority: [1; 32],
                    milestone_authority: [1; 32],
                    at: 5,
                },
                json!({ "variant": "Created", "creator": h(1), "beneficiary": h(2), "token": h(0), "total": "9",
                        "kind": { "variant": "Linear", "start": 1, "end": 2 }, "cancelable": true, "transferable": false,
                        "cancel_authority": h(1), "milestone_authority": h(1), "at": 5 }),
            ),
            (
                "Claimed",
                Event::Claimed {
                    amount: 3,
                    to_private: true,
                    claimed_total: 4,
                    at: 6,
                },
                json!({ "variant": "Claimed", "amount": "3", "to_private": true, "claimed_total": "4", "at": 6 }),
            ),
            (
                "Cancelled",
                Event::Cancelled {
                    vested_at_cancel: 7,
                    refund: 2,
                    at: 8,
                },
                json!({ "variant": "Cancelled", "vested_at_cancel": "7", "refund": "2", "at": 8 }),
            ),
            (
                "MilestoneSignalled",
                Event::MilestoneSignalled {
                    index: 1,
                    amount: 600,
                    at: 9,
                },
                json!({ "variant": "MilestoneSignalled", "index": 1, "amount": "600", "at": 9 }),
            ),
            (
                "BeneficiaryTransferred",
                Event::BeneficiaryTransferred {
                    old: [2; 32],
                    new: [3; 32],
                    at: 10,
                },
                json!({ "variant": "BeneficiaryTransferred", "old": h(2), "new": h(3), "at": 10 }),
            ),
            (
                "MadeIrrevocable",
                Event::MadeIrrevocable { at: 11 },
                json!({ "variant": "MadeIrrevocable", "at": 11 }),
            ),
        ];
        let events = idl["events"].as_array().unwrap();
        assert_eq!(events.len(), cases.len());
        for (name, event, value) in cases {
            let entry = events
                .iter()
                .find(|e| e["name"] == name)
                .unwrap_or_else(|| panic!("no event {name}"));
            let selector =
                hex::encode(&sha2::Sha256::digest(format!("vesting::{name}").as_bytes())[..8]);
            assert_eq!(entry["selector"], selector, "{name} selector");
            assert_eq!(
                hex::decode(&selector).unwrap(),
                vesting_program::selector(&event),
                "{name}: the program's selector"
            );
            let real = VestingEvent {
                schedule: a(1),
                event,
            };
            let value = json!({ "schedule": h(1), "event": value });
            assert_eq!(
                encode(&idl, &entry["type"], &value),
                borsh::to_vec(&real).unwrap(),
                "{name} data"
            );
        }
    }
}
