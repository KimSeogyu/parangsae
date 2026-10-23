# Parangsae 연구 지식 허브

**목적:** 기존 개발 에이전트에게 원문 근거, 검증 가능한 가설, 놓치기 쉬운 데이터·평가 계약을 공급한다. 이 문서는 구현 지시를 대신하거나 전략 수익성을 인증하지 않는다. 코드·테스트·CI·실거래 설정은 이 PR에서 변경하지 않는다.

## 먼저 읽을 것

1. [개발 에이전트 전달 계약](developer-handoff.md): 무엇이 사실/가설/미검증인지, 무엇부터 검증할지.
2. [사용자 지정 도서](books.md): Jiang, Quantitative Momentum, 금융의 연금술의 서지·공개 범위·적용 질문.
3. [사용자 지정 논문](papers.md): Value and Momentum Everywhere / Empirical Asset Pricing via Machine Learning.
4. [커뮤니티·원천 지도](community-map.md): 속보, 공식 거버넌스, 온체인 SQL, 매크로·공공데이터를 역할별로 분리.
5. [대체 이벤트 신호](alternative-signals.md): 12개 후보의 접근·시점·검증 및 반증 설계. 어느 후보도 OOS 거래 엣지가 확인됐다는 주장이 아니다.
6. [검색 인프라·채용 링크](infrastructure-and-job.md): AWS 원문과 현재 사양 차이, 확인 불가 공고.

## 수집 상태: 초기 공개, 아직 전체 완료 아님

확인 기준: 2026-10-23. 상태는 원문 접근·읽기·검증을 구분한다.

| ID | 사용자 자료 | 현재 범위 | 다음 단계 |
|---|---|---|---|
| U01 | Bill Jiang, Investment Strategies | 공식 소개·목차·일부 장 초록 | 유료 본문 미열람; 임의 추정 금지 |
| U02 | Quantitative Momentum | 공식 목차·1장 발췌 | 나머지 본문 미열람 |
| U03 | SSRN 2174501 | 저자 공개 최종판의 주요 절 | 원표·코드·성과 미재현 |
| U04 | mlfactor.com | 19장 전체 구조 확인, 장별 본문 검토 진행 | 장별 읽은 범위·요약 추가 |
| U05 | AWS OpenSearch 글 | 지정 글 본문과 일부 현재 공식 사양 | 검색 평가 질문을 구현 전에 검증 |
| U06 | ***REMOVED*** |
| U07 | dnpquant 블로그 | 공개 웹 접근 불완전; 사용자 제공 Markdown 175개 확보 | 175개 개별 분석 진행, 웹 전체와 혼동 금지 |
| U08 | nomore_bet categoryNo=1 | 지정 카테고리 712개 목록·본문 수집 진행 | 본문 검토/차단/메타데이터만 확인을 분리 |
| U09 | nomore_bet/221378598745 | 지정 단일 글 별도 검토 | 카테고리와 중복 식별 |
| U10 | Silverchair hhaa009 | Gu·Kelly·Xiu 논문 정본 식별·주요 절 | 토큰 URL 저장 안 함, 부록·성과 미재현 |
| U11 | 금융의 연금술 한국어 전자책 | 상품·목차·저자 공개 보조자료 | DRM 본문 미열람 |
| U12 | ansrl23/223572435384 | 지정 글 및 인용 논문 추적 | 원문 주장과 적용 제안 분리 |
| U13 | Google Calendar 월간 링크 | 연구자료 아님; 접근·수집 제외 | 사용자 명확화 없이는 일정 자료를 저장하지 않음 |

712는 지정 카테고리의 게시글 수이지 검토 완료 수가 아니다. 175는 업로드에 포함된 파일 수이지 블로그 전체 역사라는 뜻이 아니다. 공개되지 않았거나 접근할 수 없는 본문은 읽었다고 표시하지 않는다. 전문 미러나 책 대체본을 만들지 않는다.

## 현재 개발 범위와의 거리

저장소 기준은 Binance USD-M / BTCUSDT 우선, 1분~4시간 bar, 수십 분~수일 보유, paper-first다. 미국 주식 월간 팩터 논문과 한국 주식 테마 매매는 이 범위에 바로 이식할 수 없다. 강 수위·해운·날씨 같은 사례는 데이터 시점·경제적 메커니즘을 배우는 확장 연구이며, 현재 BTC 전략 구현의 필수 의존성으로 만들지 않는다.

## 독립 리뷰 연결

첫 리뷰 기준 커밋: `3852fe009076da070a174bbe2f57633e23c515a7`.

- [#9](https://github.com/KimSeogyu/parangsae/issues/9): missing Parquet partition 검증 누락.
- [#10](https://github.com/KimSeogyu/parangsae/issues/10): 리샘플링 구성 분봉의 available timestamp 검사.
- [#11](https://github.com/KimSeogyu/parangsae/issues/11): 실패/불완전 TLC 로그를 성공으로 인정하는 경로.
- [#12](https://github.com/KimSeogyu/parangsae/issues/12): flat target 이후 이전 pending 주문이 남는 경로.

개발자는 최신 main에서 재현한 뒤 처리한다. Rust 환경에서 실행하지 못한 제안 테스트는 이슈에 명시되어 있다. #11의 Python 재현과 Rust 테스트 실행을 혼동하지 않는다. 이 링크들은 안전성이나 수익성의 포괄적 검증 결과가 아니다.

## 권리·비밀·출처

공개 문서의 짧은 독자적 요약과 정본 링크만 보관한다. 공개 열람 가능성과 원문·데이터 재배포 허가는 다르다. 만료·인증 토큰 URL, 사용자 계정 정보, 불필요한 개인/가족 자료, 원본 업로드 전문은 커밋하지 않는다. 원문 설명, 작성자 주장, 독립 검증, 개발 제안은 구분한다. 이후 배치는 이 인덱스와 coverage ledger를 갱신한다.
