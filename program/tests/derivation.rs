//! R3 (second half, see docs/RELIABILITY-TESTS.md): schedule accounts never collide for one
//! creator, and one `Create` carries every schedule's nonce, through the program's own derivation
//! and encoders (the SDK's LEZ backend uses the same functions).

use std::collections::HashSet;

use lee_core::account::AccountId;
use vesting_core::ScheduleKind;
use vesting_program::{schedule_address, schedule_seed, Asset, Instruction, NewSchedule};

/// The SDK's nonce shape: a per-map tag, then the local id (little endian) in the last 8 bytes.
fn nonce(tag: &str, index: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..tag.len()].copy_from_slice(tag.as_bytes());
    out[24..].copy_from_slice(&(index as u64).to_le_bytes());
    out
}

#[test]
fn r3_schedule_derivation_never_collides_for_one_creator() {
    let creator = AccountId::new([1u8; 32]);
    let program = AccountId::new([9u8; 32]);
    let (mut nonces, mut seeds, mut addresses) = (HashSet::new(), HashSet::new(), HashSet::new());
    for index in 0..256usize {
        let n = nonce("sdk", index);
        assert!(nonces.insert(n), "nonce collided at index {index}");
        let seed = schedule_seed(&creator, &n);
        assert!(
            seeds.insert(*seed.as_bytes()),
            "seed collided at index {index}"
        );
        let address = schedule_address(&program, &creator, &n);
        assert!(
            addresses.insert(address),
            "account collided at index {index}"
        );
        // Deterministic: the same inputs give the same bytes.
        assert_eq!(schedule_address(&program, &creator, &n), address);
        // The address is the public PDA of the seed under the program.
        assert_eq!(AccountId::for_public_pda(&program, &seed), address);
    }
    let n = nonce("sdk", 0);
    assert_ne!(
        schedule_address(&program, &creator, &n),
        schedule_address(&program, &AccountId::new([7; 32]), &n)
    );
    assert_ne!(
        schedule_address(&program, &creator, &n),
        schedule_address(&AccountId::new([8; 32]), &creator, &n)
    );

    // One create of 64 schedules carries 64 distinct nonces, at fixed offsets.
    let item = |index: usize| NewSchedule {
        nonce: nonce("sdk", index),
        beneficiary: AccountId::new([2; 32]),
        asset: Asset::Native,
        total: 1,
        kind: ScheduleKind::Linear { start: 0, end: 1 },
        cancelable: None,
        transferable: None,
        cancel_authority: None,
        milestone_authority: None,
        sponsor: None,
    };
    let item_len = borsh::to_vec(&item(0)).unwrap().len();
    let bytes = borsh::to_vec(&Instruction::Create {
        schedules: (0..64).map(item).collect(),
    })
    .unwrap();
    assert_eq!(bytes[0], 0, "Instruction::Create is tag 0");
    assert_eq!(&bytes[1..5], &64u32.to_le_bytes());
    let mut wired = HashSet::new();
    for index in 0..64usize {
        let start = 5 + index * item_len;
        let wired_nonce: [u8; 32] = bytes[start..start + 32].try_into().unwrap();
        assert_eq!(wired_nonce, nonce("sdk", index));
        assert!(wired.insert(wired_nonce));
    }
}

/// The same vector the web viewer checks (`web/viewer/test/logic.test.mjs`): the public program,
/// creator 0x01.., nonce "s" padded with zeros.
#[test]
fn the_viewer_derives_the_same_schedule_address() {
    let program = AccountId::new(hex_bytes(
        "1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d",
    ));
    let mut nonce = [0u8; 32];
    nonce[0] = b's';
    let address = schedule_address(&program, &AccountId::new([1; 32]), &nonce);
    assert_eq!(
        program.to_string(),
        "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2"
    );
    assert_eq!(
        address.to_string(),
        "DatwjdUNZmUVRzAUvNvUqZ4ar5apmER3GKDdc8B1QqDQ"
    );
}

fn hex_bytes(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}
