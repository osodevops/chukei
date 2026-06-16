#!/usr/bin/env bash
set -euo pipefail

TAG="${1:-v0.2.2}"
OUT_DIR="${2:-target/release-demo-assets}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORPUS_SRC="${ROOT_DIR}/demo/query-history.csv"
CORPUS_OUT="${OUT_DIR}/chukei-${TAG}-demo-query-history.csv"
PROJECTION_OUT="${OUT_DIR}/chukei-${TAG}-demo-projection.json"
EVIDENCE_OUT="${OUT_DIR}/chukei-${TAG}-demo-projection.evidence.json"
VERIFY_OUT="${OUT_DIR}/chukei-${TAG}-demo-evidence-verify.txt"
REPLAY_OUT="${OUT_DIR}/chukei-${TAG}-demo-replay.txt"
ARCHIVE_OUT="${OUT_DIR}/chukei-${TAG}-demo-evidence.tar.gz"
SHA_OUT="${ARCHIVE_OUT}.sha256"

mkdir -p "${OUT_DIR}"
rm -f "${CORPUS_OUT}" "${PROJECTION_OUT}" "${EVIDENCE_OUT}" \
  "${VERIFY_OUT}" "${REPLAY_OUT}" "${ARCHIVE_OUT}" "${SHA_OUT}"

cp "${CORPUS_SRC}" "${CORPUS_OUT}"

if [[ -n "${CHUKEI_BIN:-}" ]]; then
  CHUKEI=("$(cd "$(dirname "${CHUKEI_BIN}")" && pwd)/$(basename "${CHUKEI_BIN}")")
else
  CHUKEI=(cargo run --quiet --manifest-path "${ROOT_DIR}/Cargo.toml" --bin chukei --)
fi

(
  cd "${OUT_DIR}"
  "${CHUKEI[@]}" replay \
    --query-history "$(basename "${CORPUS_OUT}")" \
    --output "$(basename "${PROJECTION_OUT}")" \
    --evidence \
    > "$(basename "${REPLAY_OUT}")"
  "${CHUKEI[@]}" evidence verify \
    --file "$(basename "${EVIDENCE_OUT}")" \
    > "$(basename "${VERIFY_OUT}")"
)

tar -C "${OUT_DIR}" -czf "${ARCHIVE_OUT}" \
  "$(basename "${CORPUS_OUT}")" \
  "$(basename "${PROJECTION_OUT}")" \
  "$(basename "${EVIDENCE_OUT}")" \
  "$(basename "${VERIFY_OUT}")" \
  "$(basename "${REPLAY_OUT}")"

(cd "${OUT_DIR}" && shasum -a 256 "$(basename "${ARCHIVE_OUT}")" > "$(basename "${SHA_OUT}")")

echo "demo assets written:"
echo "  ${CORPUS_OUT}"
echo "  ${PROJECTION_OUT}"
echo "  ${EVIDENCE_OUT}"
echo "  ${VERIFY_OUT}"
echo "  ${ARCHIVE_OUT}"
echo "  ${SHA_OUT}"
