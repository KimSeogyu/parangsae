Feature: Dataset version manifest

  Scenario: archive rows carry a run id and no receipt time
    Given an archive ingest of "um_kline_2021-06-01_headerless.csv" with run id "0123456789abcdef0123456789abcdef"
    Then every stored kline receipt is archive run "0123456789abcdef0123456789abcdef"
    And no stored kline receipt exposes a time

  Scenario: a v1 file is rejected
    Given a v1 parquet partition for dataset "um-kline"
    When the partition is loaded
    Then the check is "schema"
