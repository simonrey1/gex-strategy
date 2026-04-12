# Live Trading

Polls IBKR for 1-min bars, refreshes GEX from ThetaData every 60s, places bracket orders.

## Poll loop

```
1. Check market open (sleep weekends/pre-market)
2. Fetch 1-min bars from IBKR
3. Refresh GEX from ThetaData
4. Process new bars → update indicators → check SL/TP → signal → orders
5. Persist state
6. Sleep 60s
```

## Differences from backtest

| | Backtest | Live |
|---|----------|------|
| Entry price | `bar.open + slippage` | `bar.close + slippage` |
| SL/TP | Intra-bar OHLC | IBKR bracket orders |
| GEX | From parquet cache | Poll every 60s |
| Stale data | N/A | Emergency close after 3 min |

## State

`data/live/state-{ticker}.json`: signal state, position, last bar, daily PnL. Atomic writes (`.tmp` → rename). On restart: load state + replay bars for indicator warmup.

## Deployment

Needs [Podman](https://podman.io/), ThetaData Pro, IBKR account.

```bash
kube/play.sh dev   # local: IB Gateway + Theta Terminal (Podman pod)
kube/play.sh prod  # VPS: everything in one pod
```

### VPS setup (Hetzner CX22+ or similar, Fedora/Ubuntu)

```bash
# deps
dnf install -y podman git curl nmap-ncat  # (Ubuntu: apt install -y podman git curl netcat-openbsd)

# clone
git clone https://<TOKEN>@github.com/<user>/gex-strategy.git /root/gex && cd /root/gex

# credentials
cat >> ~/.bashrc <<'RC'
export TWS_USERID="..."
export TWS_PASSWORD="..."
export DASHBOARD_AUTH="user:password"
RC
source ~/.bashrc
mkdir -p .thetadata && cat > .thetadata/creds.txt <<'CREDS'
theta@email.com
theta_password
CREDS

# HTTPS (optional, needs domain A record → VPS IP)
dnf install -y certbot
firewall-cmd --add-port={80,443}/tcp --permanent && firewall-cmd --reload
certbot certonly --standalone -d your-domain.com

# launch (auto-detects certs + DASHBOARD_AUTH)
kube/play.sh prod
```

Without HTTPS, dashboard is HTTP on `:8080` (SSH tunnel: `ssh -L 8080:localhost:8080 root@<ip>`).

### Monitoring

```bash
kube/play.sh ps            # pod + container status
kube/play.sh logs          # strategy container
kube/play.sh logs-theta    # Theta Terminal
kube/play.sh status        # gateway + Theta TCP ports
```

### Updating

```bash
cd /root/gex && git pull && kube/play.sh down && kube/play.sh prod
```

### Auto-restart (systemd)

```bash
cat > /etc/systemd/system/gex-strategy.service <<'EOF'
[Unit]
Description=GEX Strategy Pod
After=network-online.target
Wants=network-online.target
[Service]
Type=oneshot
RemainAfterExit=yes
EnvironmentFile=/root/gex/.env
ExecStart=/root/gex/kube/play.sh prod
ExecStop=/root/gex/kube/play.sh down
WorkingDirectory=/root/gex
[Install]
WantedBy=multi-user.target
EOF

cat > /root/gex/.env <<'ENV'
TWS_USERID=...
TWS_PASSWORD=...
DASHBOARD_AUTH=user:password
ENV

systemctl daemon-reload && systemctl enable gex-strategy
```

Disk: GEX cache grows ~1.2 MB/day/ticker (~6.5 GB/year for 15 tickers). 40 GB disk is fine for years.
