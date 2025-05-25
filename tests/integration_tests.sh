#!/usr/bin/env bash
set -euo pipefail

echo "Running integration tests..."
mkdir -p test_outputs

fail_count=0
pass_count=0

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

for input_file in $(ls $SCRIPT_DIR/data/[0-9][0-9]-*.csv | sort); do
  [[ "$input_file" == *.expected.csv ]] && continue

  base_name="${input_file%.csv}"
  expected_file="${base_name}.expected.csv"
  output_file="test_outputs/$(basename "$base_name").out"

  echo "Testing $input_file..."

  if ! cargo run --quiet -- "$input_file" > "$output_file"; then
    echo "Runtime error on $input_file"
    fail_count=$((fail_count + 1))
    continue
  fi

  # Normalize and compare (ignore blank lines, sort rows)
  normalize() {
    grep -v '^[[:space:]]*$' "$1" | sed 's/[[:space:]]*$//' | sort
  }

  if diff <(normalize "$output_file") <(normalize "$expected_file") > /dev/null; then
    echo "✅ Passed: $(basename "$input_file")"
    pass_count=$((pass_count + 1))
  else
    echo "Output mismatch for $(basename "$input_file")"
    echo "See: diff $output_file $expected_file"
    fail_count=$((fail_count + 1))
  fi
  echo
done

echo "$pass_count passed, $fail_count failed"
exit $fail_count
