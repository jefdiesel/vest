#!/bin/bash
# Public-chain smoke of the vesting program through the CLI (no new deploy if VP is given): every
# public operation of native-LEZ schedules, with small amounts. Private claims and token schedules
# are in scripts/e2e.sh (and need more LEZ than the public faucet gives in a day).
#
# Environment: V, RPC, W (work dir), WC + C (creator wallet dir + its account, hex), WB + B, B2
# (beneficiary wallet dir + two of its public accounts, hex), VESTING_ELF (if VP unset), VP (hex).
set -uo pipefail
: "${V:?}" "${RPC:?}" "${W:?}" "${WC:?}" "${C:?}" "${WB:?}" "${B:?}" "${B2:?}"
mkdir -p "$W"; RES=$W/RESULTS.txt; MAP=$W/ids.json
export LEE_WALLET_PASSWORD=
c() { "$V" --network "$RPC" --wallet "$WC" "$@"; }
b() { "$V" --network "$RPC" --wallet "$WB" "$@"; }
nowms() { echo $(( $(date +%s) * 1000 )); }
fail=0
r() {
  local name=$1 want=$2; shift 2
  local out; out=$("$@" 2>&1); local rc=$?
  local got=ok; [ $rc = 0 ] || got=fail
  local verdict=PASS; [ "$got" = "$want" ] || { verdict=FAIL; fail=1; }
  { echo "== $(date -u +%T) $name ($verdict: want $want, got $got)"; echo "$out"; } >> "$W/log.txt"
  local tx; tx=$(echo "$out" | grep -oE 'lez: tx [0-9a-f]{64} applied in block [0-9]+ \(timestamp [0-9]+\)' | tail -1 | awk '{print $3, "block", $7, "clock", $9}' | tr -d ')')
  local line; line=$(echo "$out" | grep -E '^(claimed [0-9]|refunded|created schedule|signalled|error:|schedule .* is now|beneficiary of|program |image |claimable now|cancelled at)' | head -3 | tr '\n' ' ')
  echo "$verdict $name ${tx:+tx $tx} :: $line" | tee -a "$RES"
  LAST_OUT=$out
}
wait_claimable() { for _ in $(seq 1 60); do b "${common[@]}" claimable "$1" 2>/dev/null | grep -qE '^claimable now: [1-9]' && return 0; sleep 10; done; return 1; }
id() { echo "$LAST_OUT" | grep -oE 'created schedule [0-9]+' | head -1 | awk '{print $3}'; }
ZERO=0000000000000000000000000000000000000000000000000000000000000000

if [ -z "${VP:-}" ]; then
  r deploy-vesting ok c deploy --elf "$VESTING_ELF" --payer "$C"
  VP=$(echo "$LAST_OUT" | sed -n 's/^program //p'); [ -n "$VP" ] || exit 1
fi
echo "$VP" > "$W/program.hex"
common=(--program "$VP" --lez-map "$MAP")

echo "== linear: claim to a public account partway"
NOW=$(nowms)
r create-linear ok c "${common[@]}" create --creator "$C" --beneficiary "$B" --token $ZERO --total 600000 --kind linear --start $((NOW-30000)) --end $((NOW+300000)); L=$(id)
wait_claimable "$L"
r claim-public ok b "${common[@]}" claim "$L" --yes

echo "== cliff: refused before, claimed after; cancel; vested part claimed after the cancel"
NOW=$(nowms)
r create-cliff ok c "${common[@]}" create --creator "$C" --beneficiary "$B" --token $ZERO --total 300000 --kind cliff-linear --start $((NOW-10000)) --cliff $((NOW+60000)) --end $((NOW+600000)); K=$(id)
r claim-before-cliff fail b "${common[@]}" claim "$K" --yes
wait_claimable "$K"
r claim-after-cliff ok b "${common[@]}" claim "$K" --yes
r cancel ok c "${common[@]}" cancel "$K" --as-account "$C"
r claim-after-cancel ok b "${common[@]}" claim "$K" --yes
r show-cancelled ok b "${common[@]}" show "$K"

echo "== milestone: signal, refused repeat, irrevocable, transfer"
r create-milestone ok c "${common[@]}" create --creator "$C" --beneficiary "$B" --token $ZERO --total 200000 --kind milestone --tranche 100000 --tranche 100000 --transferable true; M=$(id)
r claim-before-signal fail b "${common[@]}" claim "$M" --yes
r signal-0 ok c "${common[@]}" signal-milestone "$M" 0 --as-account "$C"
r signal-0-again fail c "${common[@]}" signal-milestone "$M" 0 --as-account "$C"
r claim-tranche-0 ok b "${common[@]}" claim "$M" --yes
r make-irrevocable ok c "${common[@]}" make-irrevocable "$M" --as-account "$C"
r cancel-irrevocable fail c "${common[@]}" cancel "$M" --as-account "$C"
r transfer ok b "${common[@]}" transfer "$M" "$B2" --as-account "$B"
r signal-1 ok c "${common[@]}" signal-milestone "$M" 1 --as-account "$C"
r claim-new-beneficiary ok b "${common[@]}" claim "$M" --as-account "$B2" --yes
echo "== done (fail=$fail)"
exit $fail
