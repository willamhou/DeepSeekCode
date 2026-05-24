#!/usr/bin/env bash
set -euo pipefail

default_kind="python-invoice-multifile"

usage() {
  cat <<'EOF'
Usage:
  scripts/create-multifile-external-fixture.sh [root] [kind] [--force]

Kinds:
  python-invoice-multifile  Python package with pricing and invoice label edits
  rust-order-multifile     Rust crate with pricing and receipt label edits
  node-task-report         Node fixture with filtering and report heading edits

The default kind is python-invoice-multifile.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

root="${1:-/tmp/deepseek-external-fixtures/$default_kind}"
kind="${2:-$default_kind}"
force="${3:-}"

if [[ "$kind" == "--force" ]]; then
  force="--force"
  kind="$default_kind"
fi

if [[ "$force" != "" && "$force" != "--force" ]]; then
  echo "unknown argument: $force" >&2
  usage >&2
  exit 2
fi

case "$kind" in
  python-invoice-multifile|rust-order-multifile|node-task-report)
    ;;
  *)
    echo "unknown fixture kind: $kind" >&2
    usage >&2
    exit 2
    ;;
esac

if [[ -e "$root" ]]; then
  if [[ "$force" != "--force" ]]; then
    echo "fixture path already exists: $root" >&2
    echo "rerun with --force to replace it" >&2
    exit 2
  fi
  rm -rf "$root"
fi

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$command_name is required to create fixture kind '$kind'" >&2
    exit 1
  fi
}

find_python() {
  python_bin="${PYTHON:-}"
  if [[ -n "$python_bin" ]]; then
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python_bin="python3"
  elif command -v python >/dev/null 2>&1; then
    python_bin="python"
  else
    echo "python3 or python is required to create fixture kind '$kind'" >&2
    exit 1
  fi
}

commit_fixture() {
  (
    cd "$root"
    git init -q
    git config user.email "deepseek-fixture@example.invalid"
    git config user.name "DeepSeek Fixture"
    git add .
    git commit -q -m "Create $kind fixture"
  )
}

expect_initial_failure() {
  local validation_command="$1"
  local log_file="/tmp/deepseek-${evidence_slug}-fixture-test.log"

  set +e
  (
    cd "$root"
    eval "$validation_command" >"$log_file" 2>&1
  )
  local test_status=$?
  set -e

  if [[ "$test_status" -eq 0 ]]; then
    echo "expected initial fixture tests to fail, but they passed" >&2
    cat "$log_file" >&2
    exit 1
  fi
}

print_summary() {
  cat <<EOF
created: $root
kind: $kind
initial_validation: failing as expected
validation_command: $validation_command
dry_run_command: deepseek dogfood external-fixture --workdir '$root' --dry-run '$task'
evidence_command: deepseek dogfood external-fixture --workdir '$root' --evidence-out .dscode/dogfood/external-fixture-$evidence_slug-evidence.json '$task'
verify_command: deepseek dogfood external-evidence --file .dscode/dogfood/external-fixture-$evidence_slug-evidence.json --out .dscode/dogfood/external-fixture-$evidence_slug-verification.json --require-successful-external-fixtures 1
EOF
}

create_python_invoice_fixture() {
  find_python
  mkdir -p "$root/src/invoice_math" "$root/tests"

  cat > "$root/.gitignore" <<'EOF'
__pycache__/
*.pyc
EOF

  cat > "$root/src/invoice_math/__init__.py" <<'PY'
"""Small invoice fixture for DeepSeekCode external dogfood."""
PY

  cat > "$root/src/invoice_math/pricing.py" <<'PY'
def subtotal(items):
    return sum(item["quantity"] * item["unit_price"] for item in items)


def apply_discount(amount, discount):
    return amount - discount
PY

  cat > "$root/src/invoice_math/summary.py" <<'PY'
from .pricing import apply_discount, subtotal


def render_invoice(items, discount=0.0):
    total = apply_discount(subtotal(items), discount)
    return f"Invoice total: {total:.2f}"
PY

  cat > "$root/tests/test_invoice.py" <<'PY'
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))

from invoice_math.summary import render_invoice


class InvoiceSummaryTests(unittest.TestCase):
    def test_discount_is_capped_at_zero(self):
        items = [{"quantity": 1, "unit_price": 8.0}]
        self.assertEqual(render_invoice(items, discount=10.0), "Final total: 0.00")

    def test_summary_uses_final_total_label(self):
        items = [{"quantity": 2, "unit_price": 7.75}]
        self.assertEqual(render_invoice(items), "Final total: 15.50")


if __name__ == "__main__":
    unittest.main()
PY

  cat > "$root/README.md" <<'MD'
# Invoice Multi-file Fixture

Disposable external dogfood fixture for a two-file edit:

- cap discounts at zero in `src/invoice_math/pricing.py`
- rename the rendered invoice label in `src/invoice_math/summary.py`

Validation command:

```bash
python -m unittest discover -s tests
```
MD

  evidence_slug="python-invoice-multifile"
  validation_command="$python_bin -m unittest discover -s tests"
  task="replace \`return amount - discount\` with \`return max(amount - discount, 0.0)\` in src/invoice_math/pricing.py and replace \`Invoice total\` with \`Final total\` in src/invoice_math/summary.py, validate with $validation_command"
}

create_rust_order_fixture() {
  require_command cargo
  mkdir -p "$root/src" "$root/tests"

  cat > "$root/.gitignore" <<'EOF'
/target/
EOF

  cat > "$root/Cargo.toml" <<'TOML'
[package]
name = "shipping_fixture"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
TOML

  cat > "$root/src/lib.rs" <<'RS'
pub mod pricing;
pub mod receipt;

pub fn render_order(subtotal_cents: i64, discount_cents: i64, express: bool) -> String {
    let total = pricing::apply_discount(subtotal_cents, discount_cents);
    receipt::render_receipt(total, express)
}
RS

  cat > "$root/src/pricing.rs" <<'RS'
pub fn apply_discount(amount_cents: i64, discount_cents: i64) -> i64 {
    amount_cents - discount_cents
}
RS

  cat > "$root/src/receipt.rs" <<'RS'
pub fn render_receipt(total_cents: i64, _express: bool) -> String {
    let label = if _express { "Express shipping" } else { "Shipping" };
    format!("{label}: ${:.2}", total_cents as f64 / 100.0)
}
RS

  cat > "$root/tests/order_test.rs" <<'RS'
use shipping_fixture::render_order;

#[test]
fn caps_discount_at_zero() {
    assert_eq!(render_order(500, 800, false), "Delivery total: $0.00");
}

#[test]
fn uses_delivery_label_for_express_orders() {
    assert_eq!(render_order(1250, 250, true), "Delivery total: $10.00");
}
RS

  cat > "$root/README.md" <<'MD'
# Rust Order Multi-file Fixture

Disposable external dogfood fixture for a two-file Rust edit:

- cap negative order totals in `src/pricing.rs`
- normalize the rendered receipt label in `src/receipt.rs`

Validation command:

```bash
cargo test
```
MD

  (
    cd "$root"
    cargo generate-lockfile >/dev/null 2>&1
  )

  evidence_slug="rust-order-multifile"
  validation_command="cargo test"
  task="replace \`amount_cents - discount_cents\` with \`(amount_cents - discount_cents).max(0)\` in src/pricing.rs and replace \`let label = if _express { \"Express shipping\" } else { \"Shipping\" };\` with \`let label = \"Delivery total\";\` in src/receipt.rs, validate with cargo test"
}

create_node_task_report_fixture() {
  require_command node
  mkdir -p "$root/src" "$root/tests"

  cat > "$root/.gitignore" <<'EOF'
node_modules/
EOF

  cat > "$root/package.json" <<'JSON'
{
  "name": "task-report-fixture",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "test": "node tests/report.test.js"
  }
}
JSON

  cat > "$root/src/filter.js" <<'JS'
function visibleTasks(tasks) {
  return tasks;
}

module.exports = { visibleTasks };
JS

  cat > "$root/src/report.js" <<'JS'
const { visibleTasks } = require("./filter");

function renderReport(tasks) {
  const lines = visibleTasks(tasks).map((task) => `- ${task.title}`);
  return ["Task report", ...lines].join("\n");
}

module.exports = { renderReport };
JS

  cat > "$root/tests/report.test.js" <<'JS'
const assert = require("assert");
const { renderReport } = require("../src/report");

const tasks = [
  { title: "Ship CLI", archived: false },
  { title: "Remove old branch", archived: true },
];

assert.strictEqual(renderReport(tasks), "Active tasks\n- Ship CLI");
JS

  cat > "$root/README.md" <<'MD'
# Node Task Report Fixture

Disposable external dogfood fixture for a two-file Node edit:

- filter archived tasks in `src/filter.js`
- rename the report heading in `src/report.js`

Validation command:

```bash
node tests/report.test.js
```
MD

  evidence_slug="node-task-report"
  validation_command="node tests/report.test.js"
  task="replace \`return tasks;\` with \`return tasks.filter((task) => !task.archived);\` in src/filter.js and replace \`Task report\` with \`Active tasks\` in src/report.js, validate with node tests/report.test.js"
}

case "$kind" in
  python-invoice-multifile)
    create_python_invoice_fixture
    ;;
  rust-order-multifile)
    create_rust_order_fixture
    ;;
  node-task-report)
    create_node_task_report_fixture
    ;;
esac

commit_fixture
expect_initial_failure "$validation_command"
print_summary
