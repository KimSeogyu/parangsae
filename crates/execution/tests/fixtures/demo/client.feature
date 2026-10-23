Feature: Offline demo response boundary
  Requests and responses are immutable fixture data. Nothing is submitted.

  Scenario Outline: Missing and uncertain results cannot authorize a retry
    Given the recorded <fixture> fixture
    When it is parsed with HTTP status <status>
    Then the reply is <reply>

    Examples:
      | fixture        | status | reply       |
      | order          | 200    | filled      |
      | not_found      | 400    | not-found   |
      | timeout        | 400    | unknown     |
      | cancel_unknown | 400    | unknown     |
      | rejected       | 400    | refused     |
      | timeout        | 503    | unknown     |
      | timeout        | 429    | rate-limited|
      | timeout        | 418    | banned      |
