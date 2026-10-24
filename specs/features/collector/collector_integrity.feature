Feature: Collector verify scope
  A verify filter narrows which archives and rows are checked.
  It does not weaken a check inside that scope. An unknown dataset
  name or a reversed date range is a usage error.

  Scenario Outline: verify refuses changed stored metrics and book payloads only inside its scope
    Given a stored "<dataset>" daily fixture in a fresh data directory
    When the stored "<dataset>" payload differs from the archive
    And verify is limited to dataset "um-kline"
    Then verify reports no faults
    When verify is limited from "2024-01-01" to "2024-01-01"
    Then verify reports no faults
    When verify is limited to dataset "<dataset>"
    Then verify reports a conflict fault

    Examples:
      | dataset    |
      | metrics    |
      | book-depth |

  Scenario Outline: a daily fact archive cannot write rows outside its filename calendar
    When a "<dataset>" daily archive contains rows from another UTC day
    Then the archive calendar is refused before fact storage

    Examples:
      | dataset    |
      | metrics    |
      | book-depth |

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

  Scenario: default verify distinguishes a partial-month ingest from deletion
    Given a monthly June 2021 um-kline archive ingested only on 2021-06-01
    When verify has no filters
    Then verify reports no faults
    When the stored um-kline partition on 2021-06-01 is deleted
    And verify has no filters
    Then verify reports missing parquet rows

  Scenario: default verify still detects corruption after a partial-month ingest
    Given a monthly June 2021 um-kline archive ingested only on 2021-06-01
    And the stored um-kline open on 2021-06-01 differs from the archive
    When verify has no filters
    Then verify reports a conflict fault

  Scenario: verify rejects a stored minute absent from every applicable raw archive
    Given a monthly June 2021 um-kline archive ingested only on 2021-06-01
    When an extra stored um-kline minute is injected on 2021-06-01
    And verify is limited to dataset "funding-rate"
    Then verify reports no faults
    When verify is limited to dataset "um-kline" from "2021-06-02" to "2021-06-02"
    Then verify reports no faults
    When verify has no filters
    Then verify reports an unexpected stored um-kline minute

  Scenario: an archive cannot admit rows outside its named calendar coverage
    When a June 2021 monthly kline archive containing a July 1 row is ingested
    Then the archive calendar mismatch is rejected before writing Parquet

  Scenario Outline: funding payload changes fail inside the selected scope
    Given a temp store with the 2021-06-01 um-kline fixture and the September 2026 funding fixture
    When the stored funding "<field>" on 2026-09-01 differs from the archive
    And verify is limited to dataset "um-kline" from "2021-06-01" to "2021-06-01"
    Then verify reports no faults
    When verify is limited to dataset "funding-rate" from "2026-09-02" to "2026-09-02"
    Then verify reports no faults
    When verify is limited to dataset "funding-rate" from "2026-09-01" to "2026-09-01"
    Then verify reports a conflict fault

    Examples:
      | field          |
      | rate           |
      | available_ts   |
      | event_ts       |
      | schema_version |

  Scenario: a missing funding partition fails default verification after a partial-month ingest
    Given a monthly September 2026 funding archive ingested only on 2026-09-01
    When verify has no filters
    Then verify reports no faults
    When the stored funding partition on 2026-09-01 is deleted
    And verify has no filters
    Then verify reports missing parquet rows

  Scenario: a missing funding row fails verification
    Given a temp store with the 2021-06-01 um-kline fixture and the September 2026 funding fixture
    When the first stored funding row on 2026-09-01 is deleted
    And verify is limited to dataset "funding-rate" from "2026-09-01" to "2026-09-01"
    Then verify reports one missing parquet row

  Scenario Outline: an expected empty funding day rejects injected stored data
    Given a requested empty funding day with <scope> scope
    When verify is limited to dataset "funding-rate" from "2026-09-02" to "2026-09-02"
    Then verify reports no faults
    When a funding row is injected into the empty day 2026-09-02
    And verify is limited to dataset "funding-rate" from "2026-09-02" to "2026-09-02"
    Then verify reports an unexpected funding row on 2026-09-02

    Examples:
      | scope    |
      | recorded |
      | legacy   |
