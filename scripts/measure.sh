#!/bin/bash
# Cycle and size measurements (docs/COSTS.md) and the batch limit, through the CLI's --dry-run on a
# LEZ rc3 sequencer. Same environment as scripts/e2e.sh, plus VP, TP, DEF (hex: a deployed vesting
# program, token program and token definition, e.g. from an e2e run) and B, B2, P (hex: a funded
# beneficiary, a second one and a private account, all in $WB). Prints one line per measurement.
set -uo pipefail
: "${V:?}" "${RPC:?}" "${W:?}" "${VP:?}" "${TP:?}" "${DEF:?}" "${C:?}" "${B:?}" "${B2:?}" "${P:?}" "${WC:?}" "${WB:?}"
MAP=$W/measure-ids.json
common=(--program "$VP" --token-program "$TP" --lez-map "$MAP")
c() { "$V" --network "$RPC" --wallet "$WC" "${common[@]}" "$@"; }
b() { "$V" --network "$RPC" --wallet "$WB" "${common[@]}" "$@"; }
ZERO=0000000000000000000000000000000000000000000000000000000000000000
nowms() { echo $(( $(date +%s) * 1000 )); }
# m NAME cmd...: dry run, print cycles / bytes / gas, or the refusal.
m() {
  local name=$1; shift
  local out; out=$("$@" --dry-run 2>&1)
  local cyc; cyc=$(echo "$out" | grep -E "metered cycles|public cycles" | grep -oE "[0-9]+" | head -1)
  local bytes; bytes=$(echo "$out" | grep -E "wire bytes" | grep -oE "[0-9]+" | head -1)
  local gas; gas=$(echo "$out" | grep -E "declared gas" | grep -oE "[0-9]+" | head -1)
  local fee; fee=$(echo "$out" | grep -E "fee at head" | grep -oE "[0-9]+" | head -1)
  local rev; rev=$(echo "$out" | grep -E "action|REVERTS|^error" | grep -v "backend I/O error: dry run" | head -1 | sed 's/^ *//' | cut -c1-200)
  echo "$name cycles=${cyc:--} wire_bytes=${bytes:--} declared_gas=${gas:--} fee_at_head=${fee:--} ${rev}"
}
id() { echo "$1" | grep -oE 'created schedule [0-9]+' | head -1 | awk '{print $3}'; }
# The CLI's `--dry-run` is global: put it after the subcommand's own arguments.
dry() { local name=$1; shift; m "$name" "$@"; }

if [ "${BATCH_ONLY:-0}" != 1 ]; then
NOW=$(nowms)
lin=(create --creator "$C" --beneficiary "$B" --token $ZERO --total 100000000 --kind linear --start $((NOW-60000)) --end $((NOW+3600000)))
dry create-native c "${lin[@]}"
L=$(id "$(c "${lin[@]}" 2>&1)")
dry claim-public-native b claim "$L" --yes
dry claim-private-native b claim "$L" --to-private "$P" --yes
dry cancel-native c cancel "$L" --as-account "$C"

ms=(create --creator "$C" --beneficiary "$B" --token $ZERO --total 3000000 --kind milestone --tranche 1000000 --tranche 2000000 --transferable true)
dry create-milestone c "${ms[@]}"
M=$(id "$(c "${ms[@]}" 2>&1)")
dry signal-milestone c signal-milestone "$M" 0 --as-account "$C"
c signal-milestone "$M" 0 --as-account "$C" > /dev/null 2>&1
dry transfer-beneficiary b transfer "$M" "$B2" --as-account "$B"
dry make-irrevocable c make-irrevocable "$M" --as-account "$C"
dry claim-milestone b claim "$M" --yes

tok=(create --creator "$C" --beneficiary "$B" --token "$DEF" --total 100000000 --kind linear --start $((NOW-60000)) --end $((NOW+3600000)))
dry create-token c "${tok[@]}"
dry create-token-sponsored c --sponsor-deposit 20000000 --sponsor-per-claim 8000000 "${tok[@]}"
T=$(id "$(c --sponsor-deposit 20000000 --sponsor-per-claim 8000000 "${tok[@]}" 2>&1)")
dry claim-public-token b claim "$T" --yes
dry claim-private-token b claim "$T" --to-private "$P" --yes
dry cancel-token c cancel "$T" --as-account "$C"

fi
echo "== batch limit (native, then token): largest N whose create simulates"
batch() { # kind n
  python3 - "$C" "$B" "$1" "$2" "$DEF" > "$W/batch-$1-$2.json" <<'PY'
import json, sys
c, b, kind, n, d = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
tok = "0" * 64 if kind == "native" else d
print(json.dumps([{"creator": c, "beneficiary": b, "token": tok, "total": "1000",
                   "kind": {"type": "linear", "start": 1, "end": 2}} for _ in range(n)]))
PY
  m "batch-$1-$2-gas10M" c --gas-limit 10000000 create-batch "$W/batch-$1-$2.json"
}
[ "${BATCH_ONLY:-0}" = 1 ] || true
for kind in native token; do
  for n in ${BATCH_NS:-1 2 4 8 16 24 32 48 64}; do batch $kind $n; done
done
