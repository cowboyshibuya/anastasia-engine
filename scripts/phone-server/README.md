# anastasia phone server (managed cloud host)

A self-managing EC2 host that runs `anastasia serve` with the WebSocket gateway so
remote clients (including SSH clients like Termius) can drive anastasia sessions
without any laptop in the loop. Billing safety is layered and each layer has
been live-tested.

Live deployment (July 2026): AWS account `302154194530`, us-east-1,
instance `i-08214cf66cd3f80c7` (m7i-flex.large), Elastic IP `54.196.207.97`.

## Architecture

```
remote client (web or SSH, connected through Tailscale)
  │  WebSocket :7643 (pair token auth, tailnet-only)
  ▼
EC2 anastasia server ──instance role──▶ AWS Bedrock (Opus 4.6 default)
  ▲
  │ tap wake link (API Gateway → wake lambda, bearer token from URL fragment)
  │ "Pair this phone" button (lambda → SSM Run Command → `anastasia pair`)
  │
AWS Budget $10 ──▶ SNS anastasia-guard-stop ──▶ breaker lambda ──▶ stop instance
                         └──▶ email
```

## Files

| File | Deployed at | Purpose |
|---|---|---|
| `units/anastasia-serve.service` | `~ec2-user/.config/systemd/user/` (user unit, linger on) | anastasia daemon + gateway, restart always |
| `units/anastasia-pair.service` | `/etc/systemd/system/` | Legacy local pairing HTTP service, now disabled after migration to SSM |
| `units/idle-autostop.{service,timer}` | `/etc/systemd/system/` | 5-min check, poweroff after 30 min idle |
| `idle-autostop.sh` | `/usr/local/bin/` | idle = no gateway/SSH clients AND anastasia has no outbound :443 (not streaming) |
| `anastasia-pair-service.py` | `/usr/local/bin/` | Legacy tailnet-only fallback; normal pairing now uses SSM |
| `wake-lambda.py` | Lambda `anastasia-phone-wake` (behind API Gateway `8c3wp4cbag`) | wake page: starts instance, polls SSM health, generates pair codes through SSM |
| `breaker-lambda.py` | Lambda `anastasia-guard-breaker` | stops the instance; subscribed to SNS `anastasia-guard-stop` |
| `IAM-LEAST-PRIVILEGE.md` | repository documentation | scoped runtime/deploy policies and lockout-safe `jade-deploy` rotation plan |

## Server config (instance)

- `~/.anastasia-cli/config.toml`: `[provider]` default bedrock/Opus 4.6, `[gateway] enabled = true, port 7643, bind 0.0.0.0`
  (note: `~/.anastasia-cli/config.toml`, NOT `~/.config/anastasia/`)
- `~/.bashrc` env: `ANASTASIA_CLI_BEDROCK_ENABLE=1`, `AWS_REGION=us-east-1`,
  `ANASTASIA_CLI_BEDROCK_MODEL=us.anthropic.claude-opus-4-6-v1`, `ANASTASIA_CLI_GATEWAY_HOST=100.109.78.41`
- Helpers: `~/bin/jc` (anastasia with bedrock), `~/bin/phone` (attach-or-create tmux anastasia)
- `loginctl enable-linger ec2-user` so the user service runs at boot
- Instance attr `instance-initiated-shutdown-behavior=stop` so `poweroff` = stopped (not billed)

## Cost guardrails (all live-tested)

| Layer | Trigger | Action |
|---|---|---|
| idle-autostop | 30 min no clients + not streaming | instance powers itself off |
| `anastasia-bedrock-tokens-warn` | >3M input tokens / 15 min | email |
| `anastasia-bedrock-tokens-stop` | >10M input tokens / 15 min, 2 periods | breaker stops instance + email |
| AWS budget `anastasia-dev-monthly-cost` | forecast >50% / actual >80% | email |
| AWS budget `anastasia-dev-monthly-cost` | actual >100% of $10 | SNS breaker stops instance + email |

The legacy `AWS/Billing/EstimatedCharges` alarms were removed because the account was not publishing that metric. The Budget notification path is active and verified. Stopped instance cost remains approximately $6/mo for encrypted EBS plus the idle Elastic IP. The Elastic IP is retained because this existing instance needs public IPv4 for outbound Bedrock, SSM, and Tailscale connectivity when it boots; all public inbound security-group rules are closed.

## Phone flow

1. Bookmark the wake link (`https://<api-id>.execute-api.us-east-1.amazonaws.com/#t=<token>`, token stored at `~/.anastasia-cli/anastasia-phone-wake-token` on the workstation). The URL fragment is not sent in HTTP requests; JavaScript exchanges it for an `Authorization: Bearer` header and keeps it in session storage.
2. Tap it: the Lambda starts the instance and polls EC2/SSM every 5 s until ready.
3. For a compatible remote client, request a pairing code; the Lambda runs `anastasia pair` through SSM and returns the gateway address and a 6-digit code.
4. SSH fallback: connect through Tailscale to `ec2-user@100.109.78.41`, then run `phone`.

## Security notes

- Gateway `/pair` requires a live 6-digit code (5-min TTL); WS requires the bearer token minted at pairing. Tokens are stored hashed server-side.
- Wake actions require a bearer token. The bookmark stores it in the URL fragment, which browsers do not send to API Gateway. Legacy `?t=` links redirect once to the fragment form.
- The EC2 security group has no public ingress. Gateway and SSH access are tailnet-only through `anastasia-phone` (`100.109.78.41`).
- Pair generation uses SSM Run Command, so port 7644 is no longer public and the legacy pair service is disabled.
- The root EBS volume and future EBS volumes are encrypted by default.
- CloudTrail records multi-region management events to a private encrypted bucket with 90-day retention. Account-level IAM Access Analyzer and S3 Block Public Access are enabled.
- API Gateway access logs exclude query strings and authorization headers and expire after 30 days.
- IAM: the instance role has inference-only access to the configured Opus 4.6 profile plus SSM managed-instance access. Waker Lambda has start/describe EC2 plus narrowly scoped SSM command permissions. Breaker Lambda has stop/describe EC2 plus SNS publish.
- The deployment access key was rotated and the prior key is inactive. Daily maintenance can use the tested `anastasia-operator` profile and `AnastasiaOperator` role, which cannot create IAM users or terminate instances. `jade-deploy` retains its administrator attachment only as a temporary recovery path until an independent root/MFA login is verified; see `IAM-LEAST-PRIVILEGE.md`.

## Rebuild from scratch (≈15 min)

1. Launch AL2023 x86_64 with an encrypted 30GB gp3 root volume, the Bedrock + SSM instance role, and a security group with no inbound rules. Associate an Elastic IP for outbound internet while running.
2. Install anastasia, Tailscale, tmux, and git. Join the tailnet as `anastasia-phone`; set the anastasia gateway host to `100.109.78.41`.
3. Copy `units/anastasia-serve.service` and the idle-autostop unit/script; enable linger and the required services. The legacy pair service is not required.
4. Deploy `wake-lambda.py` as `waker.py`, set `WAKE_TOKEN`, `INSTANCE_ID`, and `ANASTASIA_CLI_GATEWAY_HOST=100.109.78.41`, and grant scoped EC2/SSM permissions. API Gateway HTTP API routes to the Lambda.
5. Create the SNS topics/subscriptions and connect the $10 Budget's 100% actual notification to `anastasia-guard-stop`.
6. Enable CloudTrail, Access Analyzer, account-level S3 Block Public Access, EBS encryption by default, and bounded CloudWatch log retention.
7. Test: breaker stops the host; wake link starts it; SSM reports online; tailnet `/health` works; pair button returns a code targeting the tailnet address.
