mod common;

use common::*;
use proptest::prelude::*;
use vesting_core::*;

#[derive(Debug, Clone)]
enum Action {
    Claim { t: u64, to_private: bool },
    Cancel { t: u64 },
    MakeIrrevocable,
    Transfer { to: u8, t: u64 },
    Signal { index: usize, t: u64 },
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0u64..3_000, any::<bool>()).prop_map(|(t, to_private)| Action::Claim { t, to_private }),
        (0u64..3_000).prop_map(|t| Action::Cancel { t }),
        Just(Action::MakeIrrevocable),
        (0u8..5, 0u64..3_000).prop_map(|(to, t)| Action::Transfer { to, t }),
        (0usize..4, 0u64..3_000).prop_map(|(index, t)| Action::Signal { index, t }),
    ]
}

fn apply(state: &Schedule, action: Action) -> Result<Schedule, VestingError> {
    match action {
        Action::Claim { t, to_private } => {
            claim(state, state.beneficiary, t, to_private).map(|(next, _, _)| next)
        }
        Action::Cancel { t } => cancel(state, state.cancel_authority, t).map(|(next, _, _)| next),
        Action::MakeIrrevocable => {
            make_irrevocable(state, state.cancel_authority, 1_500).map(|(next, _)| next)
        }
        Action::Transfer { to, t } => {
            transfer_beneficiary(state, state.beneficiary, [to; 32], t).map(|(next, _)| next)
        }
        Action::Signal { index, t } => {
            signal_milestone(state, state.milestone_authority, index, t).map(|(next, _, _)| next)
        }
    }
}

proptest! {
    #[test]
    fn linear_conservation_under_random_actions(
        total in 1u128..1_000_000,
        start in 0u64..500,
        span in 1u64..2_000,
        actions in prop::collection::vec(action_strategy(), 0..20),
    ) {
        let mut state = linear(total, start, start + span);
        // Compare at each state's effective time: cancellation deliberately
        // freezes vested, so a far-future comparison would be misleading.
        let mut previous_vested = state.vested_at(state.last_time);
        for action in actions {
            if let Ok(next) = apply(&state, action) {
                check_conservation(&next, 3_000);
                prop_assert!(next.claimed >= state.claimed, "claimed went down");
                prop_assert!(next.last_time >= state.last_time, "time went backwards");
                // Vested at the effective time can only grow, or stay frozen
                // after cancellation.
                let vested = next.vested_at(next.last_time);
                prop_assert!(vested >= previous_vested, "vested went down");
                previous_vested = vested;
                state = next;
            }
        }
        check_conservation(&state, 3_000);
    }

    #[test]
    fn milestone_conservation_under_random_actions(
        parts in prop::collection::vec(1u128..10_000, 1..=MAX_TRANCHES),
        actions in prop::collection::vec(action_strategy(), 0..20),
    ) {
        let total: u128 = parts.iter().sum();
        let mut state = milestone(parts);
        for action in actions {
            if let Ok(next) = apply(&state, action) {
                check_conservation(&next, 3_000);
                prop_assert!(next.claimed >= state.claimed, "claimed went down");
                prop_assert!(next.vested_at(3_000) <= total);
                state = next;
            }
        }
        check_conservation(&state, 3_000);
    }

    #[test]
    fn time_backwards_never_lowers_claimable(
        total in 1u128..1_000_000,
        times in prop::collection::vec(0u64..2_000, 1..30),
    ) {
        let mut state = linear(total, 0, 1_000);
        let mut previous = state.claimable_at(0);
        for t in times {
            let now = state.claimable_at(t);
            prop_assert!(now >= previous, "time going backwards lowered claimable");
            previous = now;
            if let Ok((next, _, _)) = claim(&state, B, t, false) {
                state = next;
                previous = state.claimable_at(t);
            }
        }
    }

    #[test]
    fn vested_is_monotone_and_bounded(
        total in 1u128..u128::MAX,
        start in 0u64..1_000,
        end_delta in 1u64..1_000_000,
        ts in prop::collection::vec(0u64..2_000_000, 1..30),
    ) {
        let end = start + end_delta;
        let state = linear(total, start, end);
        let mut ordered = ts;
        ordered.sort_unstable();
        let mut previous = 0;
        for t in ordered {
            let vested = state.vested_at(t);
            prop_assert!(vested >= previous, "vested went down at t={}", t);
            prop_assert!(vested <= total, "vested exceeded total at t={}", t);
            previous = vested;
        }
        prop_assert_eq!(state.vested_at(end), total, "vested(end) must be exact");
    }

    #[test]
    fn a_second_milestone_signal_never_unlocks_twice(
        parts in prop::collection::vec(1u128..10_000, 1..=MAX_TRANCHES),
        index in 0usize..MAX_TRANCHES,
    ) {
        let state = milestone(parts);
        if index >= state.kind.tranche_count() {
            return Ok(());
        }
        let (signalled, amount, _) = signal_milestone(&state, A, index, 1).unwrap();
        prop_assert_eq!(signalled.claimable_at(1), amount);
        prop_assert!(signal_milestone(&signalled, A, index, 2).is_err());
        prop_assert_eq!(signalled, {
            let mut again = state.clone();
            again.set_signalled(index);
            again.last_time = 1;
            again
        });
    }
}
