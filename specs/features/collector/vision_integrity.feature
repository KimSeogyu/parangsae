Feature: Vision archive integrity
  A Vision row is stored only after the declared clock, the schema, and the
  dataset checks accept it. The same key with the same payload is a no-op.
  The same key with a different payload does not replace the stored row.

  Scenario: a headerless USD-M minute passes close time, OHLC, and taker buy
    Given the kline fixture "um_kline_2021-06-01_headerless.csv" as USD-M milliseconds
    When the kline rows are checked
    Then there is no row fault

  Scenario: a close time that is not open plus the interval minus one millisecond fails
    Given the kline fixture "um_kline_bad_close.csv" as USD-M milliseconds
    When the kline rows are checked
    Then the first fault is "close_time"

  Scenario: a low above the open fails OHLC
    Given the kline fixture "um_kline_bad_ohlc.csv" as USD-M milliseconds
    When the kline rows are checked
    Then the first fault is "ohlc"

  Scenario: taker buy above volume fails
    Given the kline fixture "um_kline_bad_taker.csv" as USD-M milliseconds
    When the kline rows are checked
    Then the first fault is "taker_buy"

  Scenario: spot milliseconds on 2024-12-31 pass the close-time rule
    Given the kline fixture "spot_kline_2024-12-31_ms.csv" as spot on 2024-12-31
    When the kline rows are checked
    Then there is no row fault

  Scenario: spot microseconds on 2025-01-01 fail the millisecond close-time rule
    Given the kline fixture "spot_kline_2025-01-01_us.csv" as spot on 2025-01-01
    When the kline rows are checked
    Then the first fault is "close_time"

  Scenario: a microsecond spot file read as milliseconds fails the declared unit
    Given the kline fixture "spot_kline_2025-01-01_us.csv" as USD-M milliseconds
    Then parsing fails the "timestamp_unit" check

  Scenario: a part file name is quarantined and a daily kline name is an archive
    Given the listing fixture "listing_with_part.xml"
    When the names are classified for um-kline BTCUSDT
    Then "part-00000-example-c000.zip" is quarantined
    And "BTCUSDT-1m-2021-06-01.zip" is an archive

  Scenario: a checksum mismatch fails
    Given the zip fixture "um_kline_slice.zip" and checksum "um_kline_slice.zip.CHECKSUM.bad"
    Then the checksum check fails

  Scenario: the slice zip matches its checksum
    Given the zip fixture "um_kline_slice.zip" and checksum "um_kline_slice.zip.CHECKSUM"
    Then the checksum matches

  Scenario: a funding calc_time a few milliseconds late snaps and keeps the raw time
    Given the funding fixture "funding_2026-09-01.csv"
    Then the first funding raw time is 1788220800005000 microseconds
    And the first funding slot is 1788220800000000 microseconds

  Scenario: a funding calc_time an hour off the slot fails
    Given the funding fixture "funding_off_slot.csv"
    Then parsing fails the "funding_snap" check

  Scenario: a missing 08:00 settlement is a funding gap
    Given the funding fixture "funding_missing_slot.csv"
    When the September 2026 funding slots are checked
    Then a funding gap covers 1788220800000000 plus 8 hours

  Scenario: metrics are ordered by create_time and the publication lag is unverified
    Given the metrics fixture "metrics_2026-10-20_unsorted.csv"
    When the metrics rows are ordered
    Then the metrics rows increase by create_time
    And the metrics publication lag is unverified
    And available_ts is create_time plus 5 minutes

  Scenario: identical metrics rows collapse to one slot
    Given the metrics fixture "metrics_2020-09-01_duplicate.csv"
    When the metrics rows are ordered
    Then the metrics row count is 1

  Scenario: bookDepth accepts a complete 10-level set and a complete 12-level set
    Given the book fixture "book_2023-01-01_one_ts.csv"
    When the book rows are checked
    Then there is no row fault
    And the book level schema is 1
    Given the book fixture "book_2026-10-20_one_ts.csv"
    When the book rows are checked
    Then there is no row fault
    And the book level schema is 2

  Scenario: a timestamp that is neither level set fails
    Given the book fixture "book_bad_levels.csv"
    When the book rows are checked
    Then the first fault is "level_set"

  Scenario: book depth that is not cumulative fails
    Given the book fixture "book_bad_monotonic.csv"
    When the book rows are checked
    Then the first fault is "monotonic"

  Scenario: book notional outside the implied-price band fails
    Given the book fixture "book_bad_notional.csv"
    When the book rows are checked
    Then a fault is "notional"

  Scenario: a book span longer than 90 seconds is a gap
    Given the book fixture "book_gap_real_span.csv"
    When the book gaps on 2026-10-20 are checked
    Then a book gap is longer than 90 seconds

  Scenario: an allowlisted missing book day is not a command failure
    When coverage for missing book day 2023-02-08 is judged
    Then the gap does not fail the command
    When coverage for missing book day 2026-09-01 is judged
    Then the gap fails the command

  Scenario: a complete 1m day has no gap and removing one bar records one
    Given a complete BTCUSDT 1m day on 2026-09-01
    Then that day has no kline gap
    When the last bar is removed
    Then that day has a kline gap

  Scenario: the same kline payload is unchanged and a different open is a conflict
    Given a stored BTCUSDT kline in a fresh data directory
    When the same kline is written again
    Then the write counts as unchanged
    When a different open is written for that key
    Then the write counts as a conflict
    And the stored open is unchanged
