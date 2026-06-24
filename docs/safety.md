# Safety Policy

This software does not guarantee profit. Backtests and paper trading are evidence inputs, not promises.

Default mode is paper/dry-run. Real Binance order submission is blocked unless all live gates pass exactly:

`PARANGSAE_LIVE_TRADING_ACK=I_UNDERSTAND_THIS_CAN_LOSE_MONEY`

High leverage is not a default feature. The Balanced profile caps leverage by product family and rejects higher values unless a future `aggressive_gated` profile is explicitly designed and tested.

TradFi/stock-tracking futures fail closed when capability, trading schedule, agreement state, or index constituent data is missing/stale/contradictory.
