# Funding metadata와 검색 관심도: 제품·PIT 경계

확인: **2026-10-24 00:28 UTC**. 공식 공개 문서를 읽은 source-contract 보완이다. 시간당 리서처 검토는 collector 수집 주기나 지연 측정이 아니다. 시장자료·API 응답·RSS/CSV·holdout을 읽거나 전략을 실행하지 않았다. 기존 구현이 아래 외부 필드를 쓴다고 확인하지 않았으므로 코드 결함 보고도 아니다.

[공통 PIT 계약](public-data-pit-validation.md)을 다음 두 경우에 적용한다. 현재 내려받을 수 있는 데이터, 당시 공개됐던 정보, 우리 시스템이 실제 받은 정보는 구별해야 한다.

## 1. Funding: 현재 metadata는 과거 regime 달력이 아니다

**F01 — 공식 API 설명.** [USD-M Market Data][F01]의 `/fapi/v1/fundingInfo`는 cap/floor/interval이 조정된 symbol의 `adjustedFundingRateCap`, `adjustedFundingRateFloor`, `fundingIntervalHours`를 설명한다. 문서화된 응답에는 시행·수정·최초공개 시각이 없고 과거 기준일 조회 계약도 없다. `/fapi/v1/fundingRate`의 `fundingTime`/rate와 기간 조회는 settlement history이지 metadata 발표 이력이 아니다. 두 route는 500 requests/5min/IP 제한을 공유한다고 명시한다. 이는 문서의 schema 설명이며 실제 응답·완전성·지연·재배포 권리를 검증한 결과가 아니다.

**F02 — 현재 FAQ의 과거 규칙 설명.** [Funding FAQ][F02]는 cap/floor 도달에 따른 8h/4h→1h 변경의 시행일을 **2025-05-02 08:00 UTC**, 표시 갱신을 약 15분 이내, 해당 자동 조정의 개별 공지는 없다고 설명한다. 게시 표시는 2019-09-09 02:27, 수정 표시는 2026-03-06 07:01이며 두 표시의 timezone은 추출 본문에서 확인하지 못했다. 이 현재 FAQ는 당시 공지의 archive가 아니다. 그 안의 2025년 4월 BTC 사례는 명시된 시행일보다 앞선 설명용 예시이므로 관측 사건 목록으로 쓰지 않는다. 2026-01-02의 별도 1h→4h 규칙은 batch3 개발구간 밖이다. 실제 BTCUSDT cadence 변화는 확인하지 않았다.

**적용 제안 — 기존 연구 가정을 지지하되 확대하지 않는다.**
- [Batch3](../notes/2026-10-23-batch3-signal-candidates.md)의 B3-01/B3-05는 이미 연속된 8h grid를 요구한다. 이 문서는 그 조건·공식·그리드·trial 예산을 바꾸지 않는다. 오늘의 metadata로 과거 전체가 8h였다고 인증하지 않는다.
- Settlement grid 검증과 **regime 변경을 그때 알 수 있었는지**는 별도 질문이다. 미래 regime 이벤트를 연구한다면 당시 공개 근거와 수신 vintage가 필요하다. 공지 RSS만으로 자동 변경을 전부 잡는다고 가정하지 않는다.
- 비8h·결측 구간을 임의 8h 관측이나 0 rate로 만들지 않는다. 서로 다른 기간의 rate를 동일 단위 표본처럼 섞거나 하루 세 번 지급을 고정 가정하지 않는다. 제외 수와 실제 cashflow 비용을 따로 보고한다. 기존 비용 구현을 감사했다는 뜻은 아니다.

## 2. Google Trends: API / Explore / Trending Now를 섞지 않는다

| 근거 | 공식 설명과 확인 경계 |
|---|---|
| **GT01 — 현재 [API 안내][GT01]** | 제한된 alpha 신청을 받고, rolling 5년 및 일·주·월·년 집계를 설명한다. API 접근 승인·무가입 endpoint·요금·quota를 확인하거나 신청하지 않았다. 현재 공개 안내는 intraday 수집 가능성의 증거가 아니다. |
| **GT02 — [2025-07-24 발표][GT02]** | rolling 1,800일과 이틀 전까지의 자료를 설명한다. API는 요청 간 일관된 상대 관심도이며 절대 검색 건수가 아니다. Explore 웹 요청은 각각 0–100으로 재조정된다. 현재 GT01은 alpha·rolling·집계 단위를 확인해 주지만 **이틀 지연은 날짜가 있는 발표의 설명**이지 현재 실측/SLA가 아니다. |
| **GT03 — [데이터 FAQ][GT03]** | 표본·익명 집계이고 낮은 검색량은 0으로 표시될 수 있다. 불규칙/자동 검색이 일부 남을 수 있고 privacy noise는 특히 낮은 관심도에서 중요하다. 작은 급등을 개인 검색 활동의 증거로 삼지 않는다. 표본 재추출·정정 규모는 측정하지 않았다. |
| **GT04 — [Trending Now 도움말][GT04]** | 평균 약 10분 갱신은 SLA가 아니다. `Started`는 추세 시작, `Updated`는 현지시각의 화면 갱신이다. 관련 검색어를 묶고, 구간화된 traffic과 예측 기준 대비 증가를 표시한다. 종료 추세는 재활성화될 수 있으며 CSV/RSS export가 안내된다. 실제 export schema·보존기간·도착지연은 확인하지 않았다. |

**추론과 검증 제안.** 기록된 추세 시작은 collector의 first-seen이 아니다. Explore의 서로 다른 요청 창을 그대로 이어 붙이면 재정규화 때문에 과거 값·threshold 의미가 달라질 수 있다. 이 위험은 설명에서 도출한 것으로, 특정 BTC 시계열에서 측정한 현상이 아니다. 제품·지역·topic/query·matching·조회 창·revision·실제 수신시각을 보존하고, 존재하지 않는 export 필드는 만들어 채우지 않는다. 오늘 받은 최종 역사로 과거 공개시각이나 당시 vintage 가용성을 추정하지 않는다.

## 3. 채택 전 질문과 반증 계약

이는 **미실행 제안**이며 새로운 collector/전략 승인이나 frozen plan 변경이 아니다.

| 경계 | 최소 acceptance / 보류 조건 |
|---|---|
| 접근·권리·비용 | 실제 사용할 제품의 접근권한, 자동수집·보존·재사용 조건, quota/비용을 확인한다. 문서의 공개 열람·RSS 안내·웹페이지 콘텐츠 라이선스가 데이터 재배포 허가는 아니다. 미확인은 미확인으로 둔다. |
| PIT | source-claimed time, 확인된 first-public time, collector receipt, revision receipt를 분리한다. backfill은 과거 도착으로 소급하지 않는다. 당시 vintage/availability가 입증되지 않으면 pseudo-real-time 한계를 표시한다. |
| 선택·누락 | 고정한 제품·지역·topic/query 모집단에서 실패/빈 결과·종료/재활성화·수집 중단을 보존한다. 0, threshold 미달, 누락, 오류를 섞지 않는다. 사후 인기 사건만 선택하지 않는다. |
| 예측력 | 사전 고정한 가격·변동성·거래량·시간대 baseline과 시간순 OOS, 중복 사건 군집, 겹치는 예측구간 분리를 사용한다. 탐색한 query/지역/threshold도 시도 수에 포함한다. |
| 기각 | PIT·접근 조건이나 독립 사건 수가 부족하면 보류한다. baseline 대비 증분효과가 없거나 관측 지연에 사라지면 예측 가설을 기각한다. 거래로 전환하려면 fees/spread/slippage/실제 funding까지 포함한 별도 검증이 필요하다. |

**제한된 후속 질문:** 사전에 정한 Bitcoin 집계 관심도 cluster의 최초 수신이 다음 4h BTCUSDT 변동성에 baseline 이상의 정보를 주는가? 가격·뉴스가 검색을 유발하는 역인과를 경쟁 설명으로 둔다. 방향성 매매 규칙이 아니며 BTC 사건 수·분포·성과는 아직 모른다. 수집 권리·실제 필드·PIT 이력을 먼저 확인하고, 그 전에는 이 질문을 backtest 가능한 입력으로 표시하지 않는다. 개인·지갑 추적이나 군사 활동 추론을 하지 않는다.

## 4. 출처별 coverage ledger

모두 **2026-10-24 00:28 UTC 재열람**. 최초 검토는 F01/F02가 2026-10-23 17:23 UTC, GT01–GT04가 23:27–23:28 UTC다. 이 시각들은 리서처의 문서 열람 시각이지 source event/release 시각이 아니다. 명시하지 않은 페이지의 최초 게시·revision 시각은 확인하지 못했다.

| ID | 직접 읽은 범위 | 독립 검증하지 않은 것 |
|---|---|---|
| F01 | Funding Rate Info/History 두 문서 section과 schema | endpoint 응답·역사 coverage·실시간 SLA |
| F02 | 게시/수정 표시와 §8 interval 규칙·예시 | 당시 공지 snapshot·BTC 적용 사건 |
| GT01 | 현행 공개 API 소개 | alpha 승인·실제 접근·현재 데이터 vintage |
| GT02 | 날짜 있는 공식 발표 | 현재 이틀 지연의 실측·SLA |
| GT03 | 공식 데이터 FAQ | 표본/noise/정정의 수치 재현 |
| GT04 | 공식 Trending Now 도움말 | export schema·보존·누락·수신 지연 |

공개 원문의 짧은 자체 요약만 보관한다. 원문·이미지·원시 데이터 전문을 재배포하지 않는다. 계정/키/결제/신청, API·시장자료 수집, 코드·테스트·CI·전략·holdout 실행은 수행하지 않았다. 문서 읽기 완료와 데이터 채택·성과 검증 완료는 별개다.

[F01]: https://developers.binance.com/en/docs/catalog/core-trading-derivatives-trading-usd-s-m-futures/api/rest-api/market-data
[F02]: https://www.binance.com/en/support/faq/detail/360033525031
[GT01]: https://developers.google.com/search/apis/trends
[GT02]: https://developers.google.com/search/blog/2025/07/trends-api
[GT03]: https://support.google.com/trends/answer/4365533?hl=en
[GT04]: https://support.google.com/trends/answer/3076011?hl=en
