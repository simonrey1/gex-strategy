use super::log_debug;

pub struct AccountEquity {
    pub net_liq: f64,
    pub net_liq_currency: String,
    pub usd_available: Option<f64>,
}

impl AccountEquity {
    pub async fn fetch(client: &ibapi::Client) -> Option<Self> {
        use ibapi::accounts::types::AccountId;
        use ibapi::accounts::AccountUpdate;

        for attempt in 1..=3 {
            let accounts = match tokio::time::timeout(
                tokio::time::Duration::from_secs(10),
                client.managed_accounts(),
            )
            .await
            {
                Ok(Ok(a)) if !a.is_empty() => a,
                Ok(Ok(_)) => {
                    eprintln!("[ibkr] managed_accounts returned empty (attempt {}/3)", attempt);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    continue;
                }
                Ok(Err(e)) => {
                    eprintln!("[ibkr] managed_accounts failed (attempt {}/3): {:?}", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    continue;
                }
                Err(_) => {
                    eprintln!("[ibkr] managed_accounts timed out (attempt {}/3)", attempt);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    continue;
                }
            };

            let account_id = AccountId(accounts[0].clone());
            if attempt == 1 {
                log_debug!("[ibkr] Using account: {}", account_id.0);
            }

            let stream = match tokio::time::timeout(
                tokio::time::Duration::from_secs(10),
                client.account_updates(&account_id),
            )
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    eprintln!("[ibkr] account_updates failed (attempt {}/3): {:?}", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    continue;
                }
                Err(_) => {
                    eprintln!("[ibkr] account_updates timed out (attempt {}/3)", attempt);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    continue;
                }
            };

            let mut net_liq: Option<f64> = None;
            let mut net_liq_currency = String::new();
            let mut usd_available: Option<f64> = None;
            let mut items_seen = 0u32;
            let mut stream = stream;

            let read_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(15),
                async {
                    while let Some(update) = stream.next().await {
                        match update {
                            Ok(AccountUpdate::AccountValue(av)) => {
                                items_seen += 1;
                                let val: f64 = av.value.parse().unwrap_or(0.0);

                                if attempt == 1 && av.currency == "USD"
                                    && (av.key.contains("Cash") || av.key.contains("Funds")
                                        || av.key.contains("Liq"))
                                {
                                    log_debug!(
                                        "[ibkr] account: {}={} {}",
                                        av.key, av.value, av.currency
                                    );
                                }

                                match (av.key.as_str(), av.currency.as_str()) {
                                    ("NetLiquidation", "BASE") | ("NetLiquidation", _)
                                        if net_liq.is_none() || av.currency == "BASE" =>
                                    {
                                        net_liq = Some(val);
                                        net_liq_currency =
                                            if av.currency == "BASE" { "BASE".into() } else { av.currency };
                                    }
                                    ("AvailableFunds", "USD") => {
                                        usd_available = Some(val);
                                    }
                                    ("CashBalance", "USD") if usd_available.is_none() => {
                                        usd_available = Some(val);
                                    }
                                    _ => {}
                                }
                            }
                            Ok(AccountUpdate::End) => break,
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("[ibkr] account_updates stream error: {:?}", e);
                                break;
                            }
                        }
                    }
                },
            )
            .await;

            if read_result.is_err() {
                eprintln!("[ibkr] account_updates stream read timed out (attempt {}/3)", attempt);
            }

            if attempt == 1 {
                log_debug!(
                    "[ibkr] account_updates: {} items, net_liq={:?} {}, usd_available={:?}",
                    items_seen,
                    net_liq,
                    net_liq_currency,
                    usd_available,
                );
            }

            if net_liq.is_some() || usd_available.is_some() {
                return Some(AccountEquity {
                    net_liq: net_liq.unwrap_or(0.0),
                    net_liq_currency,
                    usd_available,
                });
            }

            eprintln!(
                "[ibkr] account_updates attempt {}/3 returned no values ({} items seen)",
                attempt, items_seen
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        None
    }
}
