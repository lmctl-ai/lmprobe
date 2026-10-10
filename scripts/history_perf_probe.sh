#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: history_perf_probe.sh [options]

Run lmprobe history_perf ignored tests in a controlled serial loop and
append output to a timestamped log, with optional post-run summary for
history_perf cold/warm history metrics.

Options:
  -n, --runs N             Number of runs (default: 30)
  --run-count N             Alias for --runs (use for readability)
  -r, --relax N            LMPROBE_PERF_RELAX value (default: 1)
  -l, --log-dir DIR        Directory for timestamped logs (default: scripts/.history_perf_logs)
  --summary-only LOGFILE    Print summary from an existing history_perf log and exit
  --summary-json LOGFILE    Print JSON summary from an existing history_perf log and exit
  -h, --help               Show this help text

Environment:
  ROOT             Root directory to run from (default: script's repo parent)
  LMPROBE_BIN      Optional path to lmprobe binary

Examples:
  ./scripts/history_perf_probe.sh
  ./scripts/history_perf_probe.sh -n 10 -r 1.0
  ./scripts/history_perf_probe.sh --runs 3 --relax 1.2
  ./scripts/history_perf_probe.sh --summary-only scripts/.history_perf_logs/history_perf_probe_20260101_120000.log

USAGE
}

RUNS=30
RELAX=1
LOG_DIR="scripts/.history_perf_logs"
ROOT=""
SUMMARY_ONLY=""
SUMMARY_JSON=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -n|--runs)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      RUNS="$2"
      shift 2
      ;;
    --run-count)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      RUNS="$2"
      shift 2
      ;;
    -r|--relax)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      RELAX="$2"
      shift 2
      ;;
    -l|--log-dir)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      LOG_DIR="$2"
      shift 2
      ;;
    --summary-only)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      SUMMARY_ONLY="$2"
      shift 2
      ;;
    --summary-json)
      if [[ $# -lt 2 ]]; then
        echo "error: $1 requires a value" >&2
        exit 1
      fi
      SUMMARY_JSON="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      usage
      exit 1
      ;;
  esac
done

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [[ "$RUNS" -lt 1 ]]; then
  echo "error: --runs must be a positive integer" >&2
  exit 1
fi

if ! [[ "$RELAX" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "error: --relax must be a positive number" >&2
  exit 1
fi

summarize_field() {
  local log_path="$1"
  local metric="$2"
  local column="$3"
  local metric_summary
  local n best median p95 max mean

  metric_summary=$(metric_stats "$log_path" "$metric" "$column")

  if [[ -z "$metric_summary" ]]; then
    echo "n=0 (no matches)"
    return
  fi

  read -r n best median p95 max mean <<< "$metric_summary"

  echo "n=$n best=$best median=$median p95=$p95 max=$max mean=$mean"
}

metric_stats() {
  local log_path="$1"
  local metric="$2"
  local column="$3"

  local values_file
  values_file=$(mktemp)
  awk -v metric="$metric" -v column="$column" '$1 == metric { print $column }' "$log_path" > "$values_file"

  local count
  count=$(wc -l < "$values_file" | tr -d '[:space:]')

  if [[ "$count" -eq 0 ]]; then
    rm -f "$values_file"
    return
  fi

  sort -n "$values_file" -o "$values_file"

  local best
  local worst
  local mean
  local median_index
  local p95_index
  best=$(head -n 1 "$values_file")
  worst=$(tail -n 1 "$values_file")

  mean=$(awk '{ sum += $1 } END { if (NR > 0) printf "%.0f", sum / NR }' "$values_file")

  if (( count % 2 == 0 )); then
    median_index=$((count / 2))
  else
    median_index=$(((count + 1) / 2))
  fi

  p95_index=$(( (count * 95 + 99) / 100 ))

  local median
  local p95
  median=$(sed -n "${median_index}p" "$values_file")
  p95=$(sed -n "${p95_index}p" "$values_file")

  rm -f "$values_file"
  echo "$count $best $median $p95 $worst $mean"
}

count_exceeding() {
  local log_path="$1"
  local metric="$2"
  local column="$3"
  local threshold="$4"

  awk -v metric="$metric" -v column="$column" -v threshold="$threshold" '$1 == metric && $column > threshold { c++ } END { print c+0 }' "$log_path"
}

metric_percentile() {
  local log_path="$1"
  local metric="$2"
  local column="$3"
  local percentile="$4"

  local values_file
  values_file=$(mktemp)
  awk -v metric="$metric" -v column="$column" '$1 == metric { print $column }' "$log_path" > "$values_file"

  local count
  count=$(wc -l < "$values_file" | tr -d '[:space:]')

  if [[ "$count" -eq 0 ]]; then
    rm -f "$values_file"
    echo 0
    return
  fi

  sort -n "$values_file" -o "$values_file"
  local index
  index=$(( (count * percentile + 99) / 100 ))

  local p
  p=$(sed -n "${index}p" "$values_file")
  rm -f "$values_file"
  echo "$p"
}

metric_matches() {
  local log_path="$1"
  local metric="$2"

  awk -v metric="$metric" '$1 == metric { c++ } END { print c+0 }' "$log_path"
}

deconcession_verdict() {
  local total="$1"
  local pass="$2"
  local p95="$3"
  local mode="${4:-compact}"

  if [[ "$total" -ne 30 ]]; then
    if [[ "$mode" == "json" ]]; then
      echo "NOT-ENOUGH-SAMPLES"
    else
      echo "verdict=NOT-ENOUGH-SAMPLES(need=30 have=$total)"
    fi
    return
  fi

  if (( pass >= 29 && p95 <= 2000 )); then
    if [[ "$mode" == "json" ]]; then
      echo "READY"
    else
      echo "verdict=READY(cold<=2000 count=$pass/30, p95=$p95)"
    fi
  else
    if [[ "$mode" == "json" ]]; then
      echo "NOT-READY"
    else
      echo "verdict=NOT-READY(cold<=2000 count=$pass/30, p95=$p95)"
    fi
  fi
}

emit_summary() {
  local log_path="$1"

  if [[ ! -f "$log_path" ]]; then
    echo "error: summary log not found: $log_path" >&2
    return 1
  fi

  echo
  echo "history_perf summary from: $log_path"
  echo "metric       cold(ms): n/best/median/p95/max/mean | warm(ms): n/best/median/p95/max/mean"
  for metric in history_blame history_introduced; do
    local cold_summary
    local warm_summary
    local cold_count_2000
    local cold_count_2200
    local total_cold
    local cold_pass_2000
    local cold_p95
    local verdict

    total_cold=$(metric_matches "$log_path" "$metric")
    cold_summary=$(summarize_field "$log_path" "$metric" 2)
    warm_summary=$(summarize_field "$log_path" "$metric" 3)
    cold_count_2000=$(count_exceeding "$log_path" "$metric" 2 2000)
    cold_count_2200=$(count_exceeding "$log_path" "$metric" 2 2200)
    cold_p95=$(metric_percentile "$log_path" "$metric" 2 95)
    cold_pass_2000=$((total_cold - cold_count_2000))
    verdict=$(deconcession_verdict "$total_cold" "$cold_pass_2000" "$cold_p95")

    printf '%-14s cold[%s] | warm[%s] | cold-gates: <=2000 %d/%d | <=2200 %d/%d | %s\n' \
      "$metric:" "$cold_summary" "$warm_summary" "$cold_pass_2000" "$total_cold" "$((total_cold-cold_count_2200))" "$total_cold" "$verdict"
  done
}

emit_summary_json() {
  local log_path="$1"

  if [[ ! -f "$log_path" ]]; then
    echo "error: summary log not found: $log_path" >&2
    return 1
  fi

  local metric
  local cold_total
  local cold_exceed_2000
  local cold_exceed_2200
  local cold_n cold_best cold_median cold_p95 cold_max cold_mean
  local warm_n warm_best warm_median warm_p95 warm_max warm_mean
  local cold_pass_2000
  local stats
  local verdict

  echo "{"
  echo '  "metrics": ['
  for metric in history_blame history_introduced; do
    cold_total=$(metric_matches "$log_path" "$metric")
    cold_exceed_2000=$(count_exceeding "$log_path" "$metric" 2 2000)
    cold_exceed_2200=$(count_exceeding "$log_path" "$metric" 2 2200)
    stats="$(metric_stats "$log_path" "$metric" 2)"
    if [[ -z "$stats" ]]; then
      cold_n=0
      cold_best=0
      cold_median=0
      cold_p95=0
      cold_max=0
      cold_mean=0
    else
      read -r cold_n cold_best cold_median cold_p95 cold_max cold_mean <<< "$stats"
    fi

    stats="$(metric_stats "$log_path" "$metric" 3)"
    if [[ -z "$stats" ]]; then
      warm_n=0
      warm_best=0
      warm_median=0
      warm_p95=0
      warm_max=0
      warm_mean=0
    else
      read -r warm_n warm_best warm_median warm_p95 warm_max warm_mean <<< "$stats"
    fi

    cold_pass_2000=$((cold_total - cold_exceed_2000))
    verdict=$(deconcession_verdict "$cold_total" "$cold_pass_2000" "$cold_p95" "json")

    printf '    {"metric":"%s","cold":{"count":%s,"best":%s,"median":%s,"p95":%s,"max":%s,"mean":%s},"warm":{"count":%s,"best":%s,"median":%s,"p95":%s,"max":%s,"mean":%s},"cold_pass_2000":%s,"cold_total":%s,"cold_pass_2200":%s,"cold_total_2200":%s,"verdict":"%s"}' \
      "$metric" "$cold_n" "$cold_best" "$cold_median" "$cold_p95" "$cold_max" "$cold_mean" \
      "$warm_n" "$warm_best" "$warm_median" "$warm_p95" "$warm_max" "$warm_mean" \
      "$cold_pass_2000" "$cold_total" "$((cold_total-cold_exceed_2200))" "$cold_total" "$verdict"

    if [[ "$metric" == "history_introduced" ]]; then
      echo
    else
      echo ','
    fi
  done
  echo '  ]'
  echo '}'
}

ROOT=${ROOT:-"$(cd -- "$(dirname "$0")/.." && pwd)"}
LOG_DIR=${LOG_DIR%/}
if [[ "$LOG_DIR" != /* ]]; then
  LOG_DIR="$ROOT/$LOG_DIR"
fi
mkdir -p "$LOG_DIR"

TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/history_perf_probe_${TIMESTAMP}.log"

if [[ -n "$SUMMARY_ONLY" ]]; then
  emit_summary "$SUMMARY_ONLY"
  exit 0
fi

if [[ -n "$SUMMARY_JSON" ]]; then
  emit_summary_json "$SUMMARY_JSON"
  exit 0
fi

{
  echo "history_perf_probe"
  echo "repo_root=$ROOT"
  echo "runs=$RUNS"
  echo "relax=$RELAX"
  echo "test_cmd=cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture --test-threads=1"
  echo "ts=$(date -Iseconds)"
  echo "log=$LOG_FILE"
  if [[ -n "${LMPROBE_BIN-}" ]]; then
    echo "LMPROBE_BIN=$LMPROBE_BIN"
  fi
  echo
} | tee "$LOG_FILE"

cd "$ROOT"

for i in $(seq 1 "$RUNS"); do
  echo "===== run $i/$RUNS =====" | tee -a "$LOG_FILE"
  if ! LMPROBE_PERF_RELAX="$RELAX" cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture --test-threads=1 2>&1 | tee -a "$LOG_FILE"; then
    echo "run $i failed" | tee -a "$LOG_FILE"
    exit 1
  fi
  echo | tee -a "$LOG_FILE"
done

echo "completed with $RUNS runs" | tee -a "$LOG_FILE"
emit_summary "$LOG_FILE"
