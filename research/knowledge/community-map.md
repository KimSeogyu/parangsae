# 이벤트 드리븐 리서치 소스 지도

확인일: 2026-10-23. 공개 웹을 조사했다. 비공개 Discord/Telegram 내부, 유료 API 종단 지연, 실제 거래수익은 검증하지 않았다. 아래는 인기·수익률 순위가 아니라 개발 에이전트의 데이터 소스 탐색 지도다. **원출처 → 재현 가능한 데이터 → 반례** 순서로 검증하고, 독립 구현이나 실거래 승인으로 해석하지 않는다.

## 1. 역할별 탐색 입구

| 소스 | 확인한 내용 / 활용 | 주의사항 |
|---|---|---|
| [Tree News](https://news.treeofalpha.com/) / [DB News](https://dbnews.com/) | Tree 공개 피드에 최신 뉴스와 원게시자 링크가 있다. DB는 공식기관·지역언론·기자 자료를 번역·태깅한다고 설명한다. 속보 발견용. | 홍보·재전달 혼재. 공급자의 속도 사례는 자기보고이며 알파 증거가 아니다. 유료 접근·가격은 별도 확인. |
| [Wu Blockchain](https://t.me/s/wublockchainenglish) | 아시아 거래소·정책 사건을 찾는 공개 취재/번역 채널. [공식 채널 목록](https://linktr.ee/wublockchain). | 독자 취재와 재전달을 구별하고 최초 관측본·수정본을 보관. 자동수집 권한·SLA 미확인. |
| [Lookonchain](https://www.lookonchain.com/feeds/78293) | 공개 글에서 신규 지갑의 거래소/마켓메이커 출금과 엔터티 링크를 확인할 수 있다. 트랜잭션 탐색 단서. | 출금은 관측, '매집'은 추론. 성공한 지갑 사례만 골라 전략 성과처럼 쓰지 않는다. |
| [Dune / hildobby](https://dune.com/queries/1941407/3202651) | 공개 SQL, 테이블 계보, 수정·실행 시각을 읽고 스테이킹/유출입 계산을 재현하는 입구. | 결과 갱신·인덱싱·캐시 지연을 별도로 기록. 공개 열람과 [API 사용 권한/비용](https://dune.com/sql-api-terms)은 다르다. |
| [DefiLlama](https://api-docs.defillama.com/) | TVL, 유입, 수수료·매출, stablecoin 및 프로젝트 이벤트. [정의](https://defillama.com/data-definitions)와 [어댑터 코드](https://github.com/orgs/DefiLlama/repositories)를 함께 확인. | TVL 변화를 순유입으로 보지 않는다. 일부 API는 유료. 현재 문서의 무료 yield API 종료 공지 등 접근정책을 수집 전에 다시 확인. |
| [Aave Governance](https://governance.aave.com/latest) / [LlamaRisk 사례](https://governance.aave.com/t/risk-stewards-cap-reductions-and-usdt-irm-change-on-aave-v3-2026-10-19/25885) | cap·금리곡선·담보·오라클 사건을 제안, 표결, 실행 tx로 분리해 추적할 수 있다. | 제안 ≠ 시행. 이해관계/DAO 지원 여부를 기록. 과거 담당자 정보를 현재에 그대로 적용하지 않는다. |
| [Flashbots Collective](https://collective.flashbots.net/t/the-mev-letter-158/6090) / [Ethereum Research](https://ethresear.ch/t/measuring-priority-fees-positions-and-transaction-classification-of-ethereum-blocks-july-2025-to-october-2026/26156) | 논문·코드·X 스레드·개발 일정과 데이터 정정 토론. 미시구조, 실행비용, 실패 사례 탐색에 적합. | 포럼 글은 독립 검증 완료 연구가 아니다. 정정과 반론을 함께 수집. |
| [Coin Metrics / Talos 연구](https://www.talos.com/insights/what-drives-stablecoin-supply-growth) | funding·carry·금리와 stablecoin 공급의 관계를 설명하는 연구. [Community API](https://docs.coinmetrics.io/api/v4/)로 일부 데이터 확인 가능. | 설명 회귀는 매매 수익 검증이 아니다. [데이터 재계산/지연 이력](https://status.coinmetrics.io/history)을 고려해 point-in-time 저장. |
| [Deribit Insights](https://insights.deribit.com/) | 사건이 IV·skew·만기구조에 얼마나 반영됐는지 보는 옵션 연구. [보고서 사례](https://insights.deribit.com/industry/crypto-derivatives-analytics-report-week-40-2026/). | 주간 후행 해석과 실시간 신호를 구별. 무료 보고서는 원시 데이터 재배포 허가가 아니다. |
| [Newsquawk](https://www.newsquawk.com/features) / [FinancialJuice](https://www.financialjuice.com/voice-player.aspx) | 중앙은행·지표·에너지 뉴스 데스크. 후자의 공개 화면에는 지연 표시가 있다. | 무료 재전달·유료 squawk·API 응답시간을 동일시하지 않는다. [요금](https://www.newsquawk.com/pricing)은 별도 재확인, 구독 미실행. |
| [Nick Timiraos](https://www.wsj.com/news/author/nick-timiraos) / [DeItaone 사례](https://x.com/DeItaone/status/2041163948888601085) | Fed 담당 기자의 취재와 통신사/정부 원문을 가리키는 헤드라인 포인터를 분리해 활용. | DeItaone를 Bloomberg 공식 계정으로 간주하지 않는다. X 최신 게시물 전체와 유료 기사 본문은 확인 범위 밖. |
| [IMF PortWatch](https://portwatch.imf.org/) | AIS 기반 항만·병목 통과 및 무역 관측. [방법론](https://www.imf.org/-/media/files/publications/wp/2025/english/wpiea2025093-print-pdf.pdf). 물리적 사건의 교차확인. | 관측 빈도와 데이터 공개·수정 빈도는 다르다. 무역 nowcast 가치 ≠ 자산 초과수익. |
| [TankerTrackers](https://demo.tankertrackers.com/) | AIS·위성·현장사진 교차검증을 표방한다. [상품](https://tankertrackers.com/faq/entry/what-products-and-services-are-available). | 고가 기업용 데이터가 포함된다. 과거 개인요금을 현재로 오인하지 말 것. 초기 무료 POC에서 제외. |
| [Quantocracy](https://quantocracy.com/) / [r/algotrading](https://www.reddit.com/r/algotrading/) / [r/quant](https://www.reddit.com/r/quant/) | 가설, 반례, 데이터·구현 장애를 찾는 공개 커뮤니티. | 게시자 신원·수익 인증·벤더 홍보를 신뢰 점수로 쓰지 않는다. 인용한 원연구와 공식 API로 이동해 검증. |

## 2. 반례를 먼저 찾는 읽기

[Flashbots의 예측시장 연구 소개](https://collective.flashbots.net/t/public-and-private-signals-in-prediction-markets/6082)는 성공/실패 거래를 함께 살펴보면 '거의 확정된 결과 매수'와 '큰 지갑 따라가기'의 성과 해석이 달라질 수 있다고 보고한다. 이는 연구자의 주장이지 이 프로젝트가 독립 재현한 결과는 아니다. 개발자는 승리한 지갑만 선택하지 말고 전체 모집단, 드문 큰 손실, 체결·수수료, 사건별 집중도를 검증해야 한다.

## 3. 수집 우선순위와 데이터 계약 제안

1. **발견:** Tree/Wu 같은 공개 피드로 후보 사건만 찾는다.
2. **확인:** 거래소·프로토콜·정부 원문과 실행 tx를 대조한다.
3. **재현:** 공식 상태 API, 공개 SQL, 온체인 이벤트, 공공 집계 자료를 우선한다.
4. **반증:** Flashbots/원논문/방법론 토론에서 실패 사례를 찾는다.

개발 에이전트에게 제안하는 최소 필드: `source_id`, `canonical_url`, `published_at`, `first_seen_at`, `received_at`, `revised_at`, `source_event_id`, `source_version`, `event_status`, `evidence_type`, `verification_status`, `raw_hash`, `license_access_status`. 프로젝트의 `EventTs`, `AvailableTs`, `ReceivedTs`와 명시적으로 매핑한다. 이 목록은 승인된 스키마나 구현 완료 상태가 아니다.

뉴스 제목의 시각을 곧바로 `AvailableTs`로 쓰면 안 된다. API 최초 관측, 웹 정정, 매체 전재 계보를 보존해야 한다. 계정 팔로어 수·유명세는 예측력 증거가 아니다. 뉴스보다 빠른 매매를 목표로 하기보다, 현재 BTCUSDT 중빈도 범위에서 사건 이후의 조건부 반응과 위험 상태를 먼저 검증한다.

## 4. 접근·운영 비용

[X 공식 가격](https://docs.x.com/x-api/getting-started/pricing)은 조회량 기반 비용을 명시한다. 게시물 read $0.005/resource 기준 단순 산술로 하루 1,000개 × 30일은 게시물 조회만 $150이며 다른 요청 비용은 별도다. 요금은 변할 수 있으므로 도입 직전 재확인한다. 이번 작업에서는 가입·구독·결제·비공개 커뮤니티 접근을 하지 않았다.

[GDELT](https://gdeltproject.org/data.html)도 광범위 뉴스 발견 후보지만 15분 단위 갱신 등 집계 지연을 초저지연 원문 도착으로 오인하면 안 된다. [DOC API 설명](https://blog.gdeltproject.org/gdelt-doc-2-0-api-debuts/)을 읽고 검색 결과의 시점·중복·보존기간을 먼저 실측할 것.

이 문서는 공개 자료의 짧은 요약과 프로젝트 적용 제안이다. 원문·데이터의 자동수집 및 재배포 권한을 부여하지 않는다.
