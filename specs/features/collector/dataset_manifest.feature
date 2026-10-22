Feature: Dataset version manifest

  Scenario: archive rows carry a run id and no receipt time
    Given an archive ingest of "um_kline_2021-06-01_headerless.csv" with run id "0123456789abcdef0123456789abcdef"
    Then every stored kline receipt is archive run "0123456789abcdef0123456789abcdef"
    And no stored kline receipt exposes a time

  Scenario: a v1 file is rejected
    Given a v1 parquet partition for dataset "um-kline"
    When the partition is loaded
    Then the check is "schema"

  Scenario: a funding row available before its event is rejected
    Given a funding row from "funding_2026-09-01.csv" with available_ts before its event_ts
    Then the funding archive is rejected with fault "available_before_event"
    And the row available_ts is left before its event_ts

  Scenario: a dry-run migration writes nothing
    Given a v1 parquet partition for dataset "um-kline"
    When receipts are migrated without --apply
    Then the data directory is unchanged

  Scenario: a stale gap cache blocks publish
    Given an archive ingest of "um_kline_2021-06-01_headerless.csv" with run id "0123456789abcdef0123456789abcdef"
    And a gap cache that says missing_file for that day
    When the day is published
    Then publish fails with "StaleGapCache"

  Scenario: an altered row fails load
    Given a published um-kline version of "um_kline_2021-06-01_headerless.csv"
    When the snapshot volume tick is increased
    Then load fails with "PartitionHashMismatch"

  Scenario: a version alias is refused
    When the version id "binance-vision.um.um-kline.BTCUSDT@latest" is parsed
    Then the version id error is "Alias"

  Scenario: a holdout range is refused
    Given a published um-kline version of "um_kline_2021-06-01_headerless.csv"
    When the version is loaded on 2025-10-01
    Then load fails with "HoldoutRefused"

  Scenario: a cache hit re-checks the file
    Given a published um-kline version of "um_kline_2021-06-01_headerless.csv"
    When the version is loaded
    And the snapshot volume tick is increased
    Then load fails with "PartitionHashMismatch"

  Scenario: an unlisted conflict fails load
    Given a published um-kline version of "um_kline_2021-06-01_headerless.csv"
    When a value conflict is appended for an in-range key
    Then load fails with "UnlistedConflict"
