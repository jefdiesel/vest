#!/bin/bash
# Live smoke test of the real LEZ backend (vesting CLI built with `--features lez`) on an rc3
# sequencer. Every chain write goes CLI -> vesting-core -> vesting-sdk LezBackend::store/submit.
#
#   S1: linear, 7M over 6 min  -> after it ends, claim "as of" into a fresh private account
#                                 (the private account pays its in-proof fee from the claim)
#   S2: cliff-linear 1M, cliff 90 s, end 30 min -> claim before the cliff (refused locally, nothing
#                                 sent), claim to the public beneficiary after the cliff, then the
#                                 creator cancels: unvested refunded, vested stays claimable
#
# Required environment:
#   V          the vesting CLI binary (built with --features lez)
#   RPC        sequencer URL, e.g. http://127.0.0.1:3290
#   WCRED      creator wallet dir (wallet_config.json + storage.json); WBENE beneficiary wallet dir
#   PROGRAM    the deployed spike vesting program, 64 hex
#   CLOCK_ELF  rc3 artifacts/lez/programs/clock.bin
#   CREATOR, BENE (public, 64 hex), PRIVATE (a private account of WBENE, 64 hex; may be empty)
#   MAP        the shared id map file (default ./vesting-lez-ids.json)
#   PROVE      optional wrapper for the private claim (e.g. "mini-lock.sh guard.sh"), word-split
set -uo pipefail
: "${V:?}" "${RPC:?}" "${WCRED:?}" "${WBENE:?}" "${PROGRAM:?}" "${CLOCK_ELF:?}" "${CREATOR:?}" "${BENE:?}" "${PRIVATE:?}"
MAP=${MAP:-$PWD/vesting-lez-ids.json}
ZERO=0000000000000000000000000000000000000000000000000000000000000000
common=(--network "$RPC" --program "$PROGRAM" --clock-elf "$CLOCK_ELF" --lez-map "$MAP")
cre() { "$V" "${common[@]}" --wallet "$WCRED" "$@"; }
ben() { "$V" "${common[@]}" --wallet "$WBENE" "$@"; }
step() { echo; echo "== $(date -u +%T) $*"; }
fail=0
check() { local rc=$1 what=$2; if [ "$rc" = 0 ]; then echo "PASS $what"; else echo "FAIL $what (exit $rc)"; fail=1; fi; }

# ONLY_S2=1: skip S1 and the private claim (re-runs of the public path without a proof).
NOW=$(( $(date +%s) * 1000 ))
if [ "${ONLY_S2:-0}" != 1 ]; then
step "1. create S1 (linear 7M over 6 min)"
out=$(cre create --creator "$CREATOR" --beneficiary "$BENE" --token "$ZERO" --total 7000000 \
  --kind linear --start "$NOW" --end $(( NOW + 360000 )) 2>&1); rc=$?; echo "$out"; check $rc "create S1"
S1=$(echo "$out" | grep -oE 'created schedule [0-9]+' | awk '{print $3}')
ben show "$S1"
fi
step "2. create S2 (cliff-linear 1M: cliff 90 s, end 30 min)"
out=$(cre create --creator "$CREATOR" --beneficiary "$BENE" --token "$ZERO" --total 1000000 \
  --kind cliff-linear --start "$NOW" --cliff $(( NOW + 90000 )) --end $(( NOW + 1800000 )) 2>&1); rc=$?; echo "$out"; check $rc "create S2"
S2=$(echo "$out" | grep -oE 'created schedule [0-9]+' | awk '{print $3}')
[ -n "$S2" ] || { echo "no schedule id"; exit 1; }
ben show "$S2"; ben claimable "$S2"

step "3. claim S2 before the cliff (must be refused, nothing sent)"
ben claim "$S2" --yes; rc=$?; [ $rc != 0 ]; check $? "pre-cliff claim refused"

step "4. wait for S2's cliff, then claim to the public beneficiary"
# Wait on the chain's clock, not this computer's: the clock holds the previous block's time.
until ben claimable "$S2" | grep -qE '^claimable now: [1-9]'; do sleep 10; done
ben claim "$S2" --yes; check $? "public claim S2"
ben show "$S2"

if [ "${ONLY_S2:-0}" != 1 ]; then
step "5. after S1 ends, claim it 'as of' the chain clock into the private account (real proof)"
until ben claimable "$S1" | grep -qE '^claimable now: 7000000 '; do sleep 10; done
ben claimable "$S1"
${PROVE:-} "$V" "${common[@]}" --wallet "$WBENE" claim "$S1" --to-private "$PRIVATE" --yes; check $? "private as-of claim S1"
ben show "$S1"
fi

step "6. the creator cancels S2: unvested refunded, vested stays claimable"
cre cancel "$S2" --as-account "$CREATOR"; check $? "cancel S2"
ben show "$S2"; ben claimable "$S2"
step "7. the beneficiary claims what vested before the cancel"
ben claim "$S2" --yes; check $? "claim S2 after cancel"
ben show "$S2"

step "done (fail=$fail)"
exit $fail
