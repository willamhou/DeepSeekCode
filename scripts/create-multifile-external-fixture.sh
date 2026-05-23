#!/usr/bin/env bash
set -euo pipefail

root="${1:-/tmp/deepseek-external-fixtures/python-invoice-multifile}"
force="${2:-}"

if [[ -e "$root" ]]; then
  if [[ "$force" != "--force" ]]; then
    echo "fixture path already exists: $root" >&2
    echo "rerun with --force to replace it" >&2
    exit 2
  fi
  rm -rf "$root"
fi

python_bin="${PYTHON:-}"
if [[ -z "$python_bin" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    python_bin="python3"
  elif command -v python >/dev/null 2>&1; then
    python_bin="python"
  else
    echo "python3 or python is required to create the fixture" >&2
    exit 1
  fi
fi

mkdir -p "$root/src/invoice_math" "$root/tests"

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

(
  cd "$root"
  git init -q
  git config user.email "deepseek-fixture@example.invalid"
  git config user.name "DeepSeek Fixture"
  git add README.md src tests
  git commit -q -m "Create invoice multi-file fixture"
)

set +e
(
  cd "$root"
  "$python_bin" -m unittest discover -s tests >/tmp/deepseek-multifile-fixture-test.log 2>&1
)
test_status=$?
set -e

if [[ "$test_status" -eq 0 ]]; then
  echo "expected initial fixture tests to fail, but they passed" >&2
  cat /tmp/deepseek-multifile-fixture-test.log >&2
  exit 1
fi

task='replace `return amount - discount` with `return max(amount - discount, 0.0)` in src/invoice_math/pricing.py and replace `Invoice total` with `Final total` in src/invoice_math/summary.py, validate with python -m unittest discover -s tests'

cat <<EOF
created: $root
initial_validation: failing as expected
validation_command: $python_bin -m unittest discover -s tests
dry_run_command: deepseek dogfood external-fixture --workdir '$root' --dry-run '$task'
evidence_command: deepseek dogfood external-fixture --workdir '$root' --evidence-out .dscode/dogfood/external-fixture-python-invoice-multifile.json '$task'
EOF
