-------------------------- MODULE PartitionFamilies --------------------------
PartitionFamilies ==
  [version |-> 1,
   calendar |-> "utc-micros-nonnegative-day-v1",
   families |->
     [KlineFact |->
        [key |-> <<"open_time">>, symbol |-> "symbol", time |-> "open_time",
         datasets |-> {"um-kline", "mark-price-kline", "index-price-kline", "premium-index-kline", "spot-kline"}],
      FundingFact |->
        [key |-> <<"funding_time">>, symbol |-> "symbol", time |-> "funding_time",
         datasets |-> {"funding-rate"}],
      MetricsFact |->
        [key |-> <<"create_time">>, symbol |-> "symbol", time |-> "create_time",
         datasets |-> {"metrics"}],
      BookFact |->
        [key |-> <<"timestamp", "percentage">>, symbol |-> "symbol", time |-> "timestamp",
         datasets |-> {"book-depth"}]]]
=============================================================================
