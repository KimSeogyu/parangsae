Feature: Supplied-data model catalogue does not imply market or execution support
  Scenario Outline: Each model calculates its own mechanism on visible synthetic inputs
    Given the supplied-data model fixture is <fixture>
    When the catalogue model public API is evaluated
    Then the model result is <result>

    Examples:
      | fixture                         | result                                  |
      | two-factor residuals            | long C short A with residual gap 60      |
      | announced surprise              | long surprise 50                        |
      | net carry                       | positive carry 60                       |
      | implied versus forecast variance | long delta-hedged variance edge 40      |
      | covered call expiry scenario    | capped option pnl 14 requiring 100       |
      | fitted later prediction         | learned score 9 long                    |

  Scenario Outline: Unusable observations are typed refusals, not neutral success
    Given the supplied-data model fixture is <fixture>
    When the catalogue model public API is evaluated
    Then the model result is <result>

    Examples:
      | fixture                   | result                     |
      | unavailable announcement  | event future refusal       |
      | stale announcement        | event stale refusal        |
      | overflowing carry         | carry overflow refusal     |
      | expiry predates contract  | expiry chronology refusal  |
      | expiry after decision     | expiry visibility refusal  |
      | in-sample prediction      | training cutoff refusal    |
      | USD-M options unsupported | unsupported USD-M options refusal |
