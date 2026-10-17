#!/usr/bin/env bash
# Prepares what docs/media/demo.tape records: a throwaway sample repository, and a scratch flow
# store and config, so the recording never touches your own flows.
#
#   docs/media/demo-setup.sh <repo> <state>
#
# <repo>   where the sample repository goes, such as ~/projects/shop; it must not exist.
# <state>  a folder for the scratch store and config; it must not exist.
#
# It writes <state>/env for the tape to source. The workers run small stand-in skills that live in
# the sample repository, so they walk through plan, implement, review and merge-request on a real
# agent without opening merge requests or publishing anything.
#
# Then, in a terminal:
#   source <state>/env && cd <repo> && flow ui
# ask the master to start the flows printed at the end, answer the agents' first-run questions
# (folder trust, Codex's hooks review), wait until the workers stand in different places, detach
# with C-space d, and run: vhs docs/media/demo.tape
set -euo pipefail

repo=${1:?usage: demo-setup.sh <repo> <state>}
state=${2:?usage: demo-setup.sh <repo> <state>}
for dir in "$repo" "$state"; do
    if [ -e "$dir" ]; then
        echo "$dir exists; pick a folder that does not" >&2
        exit 1
    fi
done
mkdir -p "$repo" "$state/config"
repo=$(cd "$repo" && pwd)
state=$(cd "$state" && pwd)

cd "$repo"
git init -q -b main

mkdir -p shop tests
cat > shop/__init__.py <<'EOF'
EOF
cat > shop/orders.py <<'EOF'
from dataclasses import dataclass, field
from datetime import date


@dataclass
class Line:
    sku: str
    quantity: int
    unit_price: float


@dataclass
class Order:
    number: str
    placed: date
    lines: list[Line] = field(default_factory=list)

    def total(self) -> float:
        return sum(line.quantity * line.unit_price for line in self.lines)
EOF
cat > shop/refunds.py <<'EOF'
from dataclasses import dataclass
from datetime import date

from shop.orders import Order


@dataclass
class Refund:
    order: Order
    amount: float
    refunded: date
EOF
cat > tests/__init__.py <<'EOF'
EOF
cat > tests/test_orders.py <<'EOF'
import unittest
from datetime import date

from shop.orders import Line, Order


class OrderTest(unittest.TestCase):
    def test_total_adds_every_line(self):
        order = Order("A-1", date(2026, 10, 1), [Line("tea", 2, 4.5), Line("cup", 1, 12.0)])
        self.assertEqual(order.total(), 21.0)


if __name__ == "__main__":
    unittest.main()
EOF
cat > README.md <<'EOF'
# shop

A small order book: orders, their lines and refunds. Run the tests with `python3 -m unittest`.
EOF

skill() {
    local name=$1 description=$2 body=$3
    for root in .claude/skills .agents/skills; do
        mkdir -p "$root/$name"
        printf -- '---\nname: %s\ndescription: %s\n---\n\n%s\n' "$name" "$description" "$body" \
            > "$root/$name/SKILL.md"
    done
}
skill demo-plan "Plans a change to the shop app. Use at the start of every request." \
"Read the code the request touches. Write PLAN.md: the change in two sentences, then three to five
tasks as checkboxes. If the request asks you to check something with the user first, ask that one
question and end your turn. Otherwise go on with the demo-build skill. Read each skill only when you
start it: flow takes reading a skill as starting its phase."
skill demo-build "Builds the change PLAN.md describes. Use after demo-plan." \
"Work through PLAN.md one task at a time: write the test first, then the code, then run
python3 -m unittest. Tick each task in PLAN.md when it passes. Then go on with the demo-review skill.
Read each skill only when you start it: flow takes reading a skill as starting its phase."
skill demo-review "Reviews the change before it ships. Use after demo-build." \
"Read your whole diff against main as a reviewer would. Fix what is wrong, run python3 -m unittest
again, then go on with the demo-ship skill. Read each skill only when you start it: flow takes
reading a skill as starting its phase."
skill demo-ship "Ships the change. Use after demo-review." \
"Commit the work with a short message. End your turn with one line saying the branch is ready."

mkdir -p .claude
# An empty status line keeps your own, with its account details, out of the recording.
cat > .claude/settings.json <<'EOF'
{
  "permissions": {
    "allow": ["Edit", "Write", "Bash(python3 -m unittest:*)", "Bash(git add:*)", "Bash(git commit:*)"]
  },
  "statusLine": { "type": "command", "command": "true" }
}
EOF

git add .
git commit -q -m "shop: orders and refunds"

cat > "$state/config/config.toml" <<'EOF'
[agents.claude]
command = ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
prompt = "/demo-plan {request}"
resume = ["claude", "-n", "{name}", "--resume", "{id}"]
adapter = "claude-code"

[agents.codex]
command = ["codex", "{prompt}"]
prompt = "$demo-plan {request}"
resume = ["codex", "resume", "{session}"]
adapter = "codex"

[[phases]]
name = "plan"
skills = ["demo-plan"]

[[phases]]
name = "implement"
skills = ["demo-build"]

[[phases]]
name = "review"
skills = ["demo-review"]

[[phases]]
name = "merge-request"
skills = ["demo-ship"]
EOF
cat > "$state/config/theme.toml" <<'EOF'
extends = "flow"
EOF

cat > "$state/env" <<EOF
export FLOW_HOME="$state/store"
export XDG_STATE_HOME="$state/xdg-state"
export FLOW_CONFIG="$state/config/config.toml"
unset FLOW_ID FLOW_NAME FLOW_SLOT FLOW_REPO FLOW_WORKTREE FLOW_MASTER
EOF

cat <<EOF
Ready. Start flow ui on the scratch store:

  source $state/env && cd $repo && flow ui

and ask the master:

  Start four flows: "SHOP-217 export orders and refunds as CSV, JSON and a monthly summary
  report" in auto mode; "SHOP-221 refund totals: ask me first whether refunds count on the
  order's date or the refund's date" guided; "SHOP-230 email a receipt when an order is placed,
  with a plain text and an HTML template" with Codex; "SHOP-198 round order totals to cents" in
  auto mode.
EOF
