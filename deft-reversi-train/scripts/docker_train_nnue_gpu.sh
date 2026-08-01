#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

run_in_container() {
  local env_names=(
    DATA RUN_DIR EPOCHS BATCH_SIZE LR VALID_LIMIT NUM_WORKERS PROGRESS_EVERY_STEPS
    TRAIN_LIMIT PHASE_RANGE HUBER_DELTA WEIGHT_DECAY SEED SAVE_EVERY_EPOCHS PREFETCH_FACTOR
    CHECKPOINT RESUME DEVICE LOG_DIR OUT_JSON OUT_BIN EVAL_NAME EVAL_VERSION
  )
  local docker_env_args=()
  local name
  for name in "${env_names[@]}"; do
    if [[ -v "${name}" ]]; then
      docker_env_args+=("-e" "${name}=${!name}")
    fi
  done

  docker compose -f "${REPO_ROOT}/docker-compose.train.yml" run --rm \
    "${docker_env_args[@]}" \
    deft-reversi-train \
    bash -lc "deft-reversi-train/scripts/docker_train_nnue_gpu.sh --inside-container"
}

if [[ "${1:-}" != "--inside-container" ]]; then
  if [[ "${DEFT_REVERSI_TRAIN_DOCKER:-}" == "1" ]]; then
    set -- "--inside-container" "$@"
  else
    run_in_container
    exit 0
  fi
fi

cd /workspace/deft-reversi

DATA="${DATA:-/workspace/deft-reversi/egaroucid_rd_data_set}"
RUN_DIR="${RUN_DIR:-runs/nnue-gpu}"
EPOCHS="${EPOCHS:-3}"
BATCH_SIZE="${BATCH_SIZE:-4096}"
LR="${LR:-16}"
VALID_LIMIT="${VALID_LIMIT:-200000}"
NUM_WORKERS="${NUM_WORKERS:-8}"
PREFETCH_FACTOR="${PREFETCH_FACTOR:-4}"
PROGRESS_EVERY_STEPS="${PROGRESS_EVERY_STEPS:-100}"
DEVICE="${DEVICE:-cuda}"
LOG_DIR="${LOG_DIR:-${RUN_DIR}/logs}"
OUT_JSON="${OUT_JSON:-${RUN_DIR}/nnue-eval.json}"
OUT_BIN="${OUT_BIN:-${RUN_DIR}/nnue-eval.bin}"
CHECKPOINT="${CHECKPOINT:-${RUN_DIR}/checkpoint.pt}"
EVAL_NAME="${EVAL_NAME:-nnue-gpu}"
EVAL_VERSION="${EVAL_VERSION:-2}"

mkdir -p "${RUN_DIR}" "${LOG_DIR}"

train_args=(
  --data "${DATA}"
  --out-json "${OUT_JSON}"
  --checkpoint "${CHECKPOINT}"
  --epochs "${EPOCHS}"
  --batch-size "${BATCH_SIZE}"
  --lr "${LR}"
  --valid-limit "${VALID_LIMIT}"
  --device "${DEVICE}"
  --num-workers "${NUM_WORKERS}"
  --prefetch-factor "${PREFETCH_FACTOR}"
  --log-dir "${LOG_DIR}"
  --progress-every-steps "${PROGRESS_EVERY_STEPS}"
)

if [[ -n "${TRAIN_LIMIT:-}" ]]; then
  train_args+=(--train-limit "${TRAIN_LIMIT}")
fi

if [[ -n "${PHASE_RANGE:-}" ]]; then
  train_args+=(--phase-range "${PHASE_RANGE}")
fi

if [[ -n "${HUBER_DELTA:-}" ]]; then
  train_args+=(--huber-delta "${HUBER_DELTA}")
fi

if [[ -n "${WEIGHT_DECAY:-}" ]]; then
  train_args+=(--weight-decay "${WEIGHT_DECAY}")
fi

if [[ -n "${SEED:-}" ]]; then
  train_args+=(--seed "${SEED}")
fi

if [[ -n "${SAVE_EVERY_EPOCHS:-}" ]]; then
  train_args+=(--save-every-epochs "${SAVE_EVERY_EPOCHS}")
fi

if [[ -n "${RESUME:-}" ]]; then
  if [[ "${RESUME}" == "1" || "${RESUME}" == "true" ]]; then
    train_args+=(--resume "${CHECKPOINT}")
  else
    train_args+=(--resume "${RESUME}")
  fi
fi

python3 deft-reversi-train/scripts/train_nnue_torch.py "${train_args[@]}"

cargo run -p deft-reversi-train --release -- pack-nnue \
  --input "${OUT_JSON}" \
  --out "${OUT_BIN}" \
  --eval-name "${EVAL_NAME}" \
  --eval-version "${EVAL_VERSION}"

python3 deft-reversi-train/scripts/plot_training_metrics.py \
  "${LOG_DIR}/metrics.csv" \
  --out "${LOG_DIR}/training_metrics.svg"

printf 'wrote %s\n' "${OUT_JSON}"
printf 'wrote %s\n' "${OUT_BIN}"
printf 'logs: %s/metrics.csv %s/metrics.jsonl\n' "${LOG_DIR}" "${LOG_DIR}"
printf 'graphs: %s/training_metrics.svg %s/valid_by_empties.svg\n' "${LOG_DIR}" "${LOG_DIR}"
