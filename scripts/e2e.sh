#!/bin/bash
# End to end on a LEZ v0.4.0-rc3 sequencer, through the vesting CLI only (built with --features lez):
# deploy, native and token schedules, every operation, refusals, a batch, private claims, and the
# pre-claim summary's simulated fee (public, private native, private token).
#
# Environment:
#   V            the vesting CLI binary
#   RPC          sequencer URL (a local standalone sequencer; dev-mode receipts are fine)
#   W            work dir (wallets, id map, outputs); created
#   VESTING_ELF  the vesting program guest ELF;  TOKEN_ELF  the 0.4 token program guest ELF
#   PAYER_KEY    private key (hex) of a funded genesis account, imported as the creator
#                (LEZ's published local-test key by default), or CREATOR_WALLET + CREATOR: an
#                existing wallet dir and its funded public account (base58)
#   CLOCK_ELF    optional: rc3 artifacts/lez/programs/clock.bin (else read from the chain)
#   PROVE        optional wrapper for private claims (e.g. "mini-lock.sh guard.sh")
# Writes $W/program.hex (the vesting program id) for the event check, and $W/RESULTS.txt.
set -uo pipefail
: "${V:?}" "${RPC:?}" "${W:?}" "${VESTING_ELF:?}" "${TOKEN_ELF:?}"
PAYER_KEY=${PAYER_KEY:-10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d}
mkdir -p "$W"; RES=$W/RESULTS.txt; : > "$RES"
WC=${CREATOR_WALLET:-$W/wallet-creator}; WB=$W/wallet-bene; MAP=$W/ids.json
export LEE_WALLET_PASSWORD=
clock=(); [ -n "${CLOCK_ELF:-}" ] && clock=(--clock-elf "$CLOCK_ELF")
c() { "$V" --network "$RPC" --wallet "$WC" "$@"; }
b() { "$V" --network "$RPC" --wallet "$WB" "$@"; }
hex() { python3 -c 'import sys
A="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
n=0
for ch in sys.argv[1]: n=n*58+A.index(ch)
print(n.to_bytes(32,"big").hex())' "$1"; }
b58() { grep -oE "$1/[1-9A-HJ-NP-Za-km-z]{32,44}" | head -1 | cut -d/ -f2; }
nowms() { echo $(( $(date +%s) * 1000 )); }
fail=0
# r NAME want(ok|fail) cmd...: run, log, compare with the expected outcome.
r() {
  local name=$1 want=$2; shift 2
  local out; out=$("$@" 2>&1); local rc=$?
  local got=ok; [ $rc = 0 ] || got=fail
  local verdict=PASS; [ "$got" = "$want" ] || { verdict=FAIL; fail=1; }
  { echo "== $name ($verdict: want $want, got $got)"; echo "$out"; } >> "$W/log.txt"
  local tx; tx=$(echo "$out" | grep -oE 'lez: tx [0-9a-f]{64} applied in block [0-9]+' | tail -1 | awk '{print $3, "block", $7}')
  local line; line=$(echo "$out" | grep -E '^(claimed [0-9]|refunded|created schedule|signalled|error:|schedule .* is now|beneficiary of|program |definition |estimated fee|fee check)' | head -2 | tr '\n' ' ')
  echo "$verdict $name ${tx:+tx $tx} :: $line" | tee -a "$RES"
  LAST_OUT=$out
}
# fee_ok cmd...: a pre-claim summary with a real simulated fee (> 0) that its payer can pay.
fee_ok() {
  local out; out=$("$@" 2>&1); local rc=$?; echo "$out"
  [ $rc = 0 ] && echo "$out" | grep -qE '^estimated fee: [1-9][0-9]* ' && echo "$out" | grep -q '^fee check: ok'
}
wait_claimable() { # id: until the chain says something is claimable
  for _ in $(seq 1 60); do b "${common[@]}" claimable "$1" 2>/dev/null | grep -qE '^claimable now: [1-9]' && return 0; sleep 5; done; return 1; }

echo "== setup in $W"
if [ -z "${CREATOR:-}" ]; then
  c wallet account import public --private-key "$PAYER_KEY" > "$W/import.out" 2>&1 || { cat "$W/import.out"; exit 1; }
  CREATOR=$(b58 Public < "$W/import.out")
fi
b wallet check-health > /dev/null 2>&1
acct() { b wallet account new "$1" 2>&1; }
o=$(acct public); B=$(echo "$o" | b58 Public); BPK=$(echo "$o" | sed -n 's/^With pk //p')
o=$(acct public); B2=$(echo "$o" | b58 Public); B2PK=$(echo "$o" | sed -n 's/^With pk //p')
P1=$(acct private | b58 Private); P2=$(acct private | b58 Private)
for pk in $BPK $B2PK; do c wallet auth-transfer send --from "Public/$CREATOR" --to-pk "$pk" --amount 50000000 > /dev/null 2>&1 || { echo "funding failed"; exit 1; }; done
C=$(hex "$CREATOR"); BH=$(hex "$B"); B2H=$(hex "$B2"); P1H=$(hex "$P1"); P2H=$(hex "$P2")
echo "creator $CREATOR beneficiary $B new beneficiary $B2 private $P1 $P2" | tee -a "$RES"

r deploy-vesting ok c deploy --elf "$VESTING_ELF" --payer "$C"; VP=$(echo "$LAST_OUT" | sed -n 's/^program //p')
r deploy-token ok c deploy --elf "$TOKEN_ELF" --payer "$C"; TP=$(echo "$LAST_OUT" | sed -n 's/^program //p')
[ -n "$VP" ] && [ -n "$TP" ] || { echo "deploy failed"; exit 1; }
echo "$VP" > "$W/program.hex"; echo "$TP" > "$W/token-program.hex"
common=(--program "$VP" --token-program "$TP" --lez-map "$MAP" ${clock[@]+"${clock[@]}"})
r token-new ok c "${common[@]}" token-new --holder "$C" --supply 1000000000000; DEF=$(echo "$LAST_OUT" | sed -n 's/^definition //p')
r token-init-B ok b "${common[@]}" token-init --definition "$DEF" --account "$BH"
ZERO=0000000000000000000000000000000000000000000000000000000000000000
id() { echo "$LAST_OUT" | grep -oE 'created schedule [0-9]+' | head -1 | awk '{print $3}'; }

echo "== native linear: public claim, private claim into a fresh account"
NOW=$(nowms)
r create-linear ok c "${common[@]}" create --creator "$C" --beneficiary "$BH" --token $ZERO --total 500000000 --kind linear --start $((NOW-60000)) --end $((NOW+120000)); L=$(id)
r summary-public ok fee_ok b "${common[@]}" summary "$L"
r claim-public ok b "${common[@]}" claim "$L" --yes
r summary-private-native ok fee_ok b "${common[@]}" summary "$L" --to-private "$P1H"
r claim-private-native ok ${PROVE:-} "$V" --network "$RPC" --wallet "$WB" "${common[@]}" claim "$L" --to-private "$P1H" --yes
r balance-P1 ok b "${common[@]}" balance "$P1H"

echo "== cliff: refused before, claimed after; cancel refunds, vested stays claimable"
NOW=$(nowms)
r create-cliff ok c "${common[@]}" create --creator "$C" --beneficiary "$BH" --token $ZERO --total 10000000 --kind cliff-linear --start $((NOW-10000)) --cliff $((NOW+40000)) --end $((NOW+600000)); K=$(id)
r claim-before-cliff fail b "${common[@]}" claim "$K" --yes
wait_claimable "$K"
r claim-after-cliff ok b "${common[@]}" claim "$K" --yes
r cancel ok c "${common[@]}" cancel "$K" --as-account "$C"
r cancel-again fail c "${common[@]}" cancel "$K" --as-account "$C"
r claim-after-cancel ok b "${common[@]}" claim "$K" --yes
r show-cancelled ok b "${common[@]}" show "$K"

echo "== milestone: signal, idempotency, irrevocable, transfer"
r create-milestone ok c "${common[@]}" create --creator "$C" --beneficiary "$BH" --token $ZERO --total 5000000 --kind milestone --tranche 3000000 --tranche 2000000 --transferable true; M=$(id)
r claim-before-signal fail b "${common[@]}" claim "$M" --yes
r signal-0 ok c "${common[@]}" signal-milestone "$M" 0 --as-account "$C"
r signal-0-again fail c "${common[@]}" signal-milestone "$M" 0 --as-account "$C"
r claim-tranche-0 ok b "${common[@]}" claim "$M" --yes
r make-irrevocable ok c "${common[@]}" make-irrevocable "$M" --as-account "$C"
r cancel-irrevocable fail c "${common[@]}" cancel "$M" --as-account "$C"
r transfer ok b "${common[@]}" transfer "$M" "$B2H" --as-account "$BH"
r signal-1 ok c "${common[@]}" signal-milestone "$M" 1 --as-account "$C"
r claim-old-beneficiary fail b "${common[@]}" claim "$M" --as-account "$BH" --yes
r claim-new-beneficiary ok b "${common[@]}" claim "$M" --as-account "$B2H" --yes

echo "== separate authorities: only they cancel and signal; the refund still goes to the creator"
r create-authorities ok c "${common[@]}" create --creator "$C" --beneficiary "$BH" --token $ZERO --total 4000000 --kind milestone --tranche 1000000 --tranche 3000000 --cancel-authority "$B2H" --milestone-authority "$B2H"; A=$(id)
r signal-by-creator fail c "${common[@]}" signal-milestone "$A" 0 --as-account "$C"
r signal-by-authority ok b "${common[@]}" signal-milestone "$A" 0 --as-account "$B2H"
r cancel-by-creator fail c "${common[@]}" cancel "$A" --as-account "$C"
r cancel-by-authority ok b "${common[@]}" cancel "$A" --as-account "$B2H"
r claim-after-authority-cancel ok b "${common[@]}" claim "$A" --yes

echo "== batch: three schedules in one transaction"
NOW=$(nowms)
python3 - "$C" "$BH" "$B2H" $NOW > "$W/batch.json" <<'PY'
import json, sys
c, b, b2, now = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
z = "0" * 64
print(json.dumps([
  {"creator": c, "beneficiary": b, "token": z, "total": "1000000", "kind": {"type": "linear", "start": now, "end": now + 600000}},
  {"creator": c, "beneficiary": b2, "token": z, "total": "2000000", "kind": {"type": "cliff-linear", "start": now, "cliff": now + 60000, "end": now + 600000}},
  {"creator": c, "beneficiary": b, "token": z, "total": "3000000", "kind": {"type": "milestone", "tranches": ["1000000", "2000000"]}},
]))
PY
r create-batch ok c "${common[@]}" create-batch "$W/batch.json"

echo "== token: escrow holding, public claim, private claim into a fresh account (sponsored), cancel"
NOW=$(nowms)
r create-token ok c "${common[@]}" --sponsor-deposit 30000000 --sponsor-per-claim 12000000 create --creator "$C" --beneficiary "$BH" --token "$DEF" --total 600000000 --kind linear --start $((NOW-60000)) --end $((NOW+600000)); T=$(id)
r claim-token-public ok b "${common[@]}" claim "$T" --yes
r summary-token-private ok fee_ok b "${common[@]}" summary "$T" --to-private "$P2H"
r claim-token-private ok ${PROVE:-} "$V" --network "$RPC" --wallet "$WB" "${common[@]}" claim "$T" --to-private "$P2H" --yes
r balance-P2 ok b "${common[@]}" balance "$P2H"
r cancel-token ok c "${common[@]}" cancel "$T" --as-account "$C"
r claim-token-rest ok b "${common[@]}" claim "$T" --yes
r balance-creator ok c "${common[@]}" balance "$C"

echo "== done (fail=$fail); log $W/log.txt"
exit $fail
