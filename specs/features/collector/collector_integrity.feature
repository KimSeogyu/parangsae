Feature: Collector verify scope
  A verify filter narrows which archives and rows are checked.
  It does not weaken a check inside that scope. An unknown dataset
  name or a reversed date range is a usage error.

  Scenario: verify can be limited to one dataset and a date range
    Given a temp store with the 2021-06-01 um-kline fixture and the September 2026 funding fixture
    And the stored um-kline open on 2021-06-01 differs from the archive
    When verify is limited to dataset "funding-rate"
    Then verify reports no faults
    And verify checked only the funding-rate archives
    When verify is limited to dataset "um-kline" from "2021-06-01" to "2021-06-01"
    Then verify reports a conflict fault
    When verify is asked for dataset "not-a-dataset"
    Then verify rejects the request as usage
    When verify is limited from "2021-06-02" to "2021-06-01"
    Then verify rejects the request as usage

  Scenario: a missing in-scope kline partition fails verify
    Given a temp store with the 2021-06-01 um-kline fixture and the September 2026 funding fixture
    When verify is limited to dataset "um-kline" from "2021-06-01" to "2021-06-01"
    Then verify reports no faults
    When the stored um-kline partition on 2021-06-01 is deleted
    And verify is limited to dataset "um-kline" from "2021-06-01" to "2021-06-01"
    Then verify reports missing parquet rows
