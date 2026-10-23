# 사용자 지정 자료 coverage 명세

**아직 전체 수집 완료가 아니다.** 아래는 2026-10-23의 출처별 확인 범위다. 읽기 완료와 검증·재현·재배포 허가는 별개다. 기계 판독용 [JSON ledger](user-source-coverage.json)와 각 노트의 제한을 함께 읽는다.

| ID | 자료 | 실제 확인 범위 | 남은 범위 |
|---|---|---|---|
| U01 | [Investment Strategies: A Practical Approach to Enhancing Investor Returns](books.md) | 공식 소개·16장 색인·16장 공개 초록 전부. 구독 본문은 0장. | 저작권 있는 전체 도서의 미러 없음. 정확한 구현식 확인은 합법적 본문 접근 필요. |
| U02 | [Quantitative Momentum: A Practitioner’s Guide to Building a Momentum-Based Stock Selection System](books.md) | 공식 소개·목차 및 1장 발췌 11쪽. 2–9장·부록 및 정오표 본문 미검토. | 정식 책 본문이나 공식 추가 발췌 없이는 규칙을 추정해 채우지 않음. |
| U03 | [Value and Momentum Everywhere](papers.md) | SSRN 403. 저자 공개 최종판의 초록·신호/포트폴리오·비용/강건성 주요 절. 전체 표·부록 재현 없음. | 원논문 데이터와 업데이트 데이터의 구분만 확인; 파일 다운로드·검산 없음. |
| U04 | [Machine Learning for Factor Investing](mlfactor.md) | 19장 본문 텍스트·표시 수식/표·코드/출력 정적 검토. 모든 그림 픽셀 검수 및 연결된 참고문헌 전수 읽기는 아님. | Python 색인15링크만 확인; 노트북 본문·실행 미검토. 확인한 서문/공식 저장소 루트/README에서 재배포 라이선스 미확인. |
| U05 | [Amazon OpenSearch Service의 검색 엔진으로서의 기능 설명](infrastructure-and-job.md) | 지정 글 본문과 현재 공식 엔진·거리점수·필터링 문서 대조. | 관리형 서비스의 실제 배포 버전과 이용 조건은 구현 전에 별도 확인. |
| U06 | ***REMOVED*** |
| U07 | [dnpquant — 두부](investor-dnpquant/README.md) | 공개웹 열거0·본문0. 사용자 업로드175파일 전수 확인: 식별 본문173, 본문 없음2, 명확한 부분본문2. | 전체 블로그 역사/완전 원문 확보는 아님. 5동일 합본에서5ID 복구; 말미결손 의심2·상대날짜1·이미지0검수 등은 archive coverage 참조. |
| U08 | [nomore_bet — 금융공학 우회도로(categoryNo=1)](naver-sources.md) | 공식 24페이지에서714 unique 글ID 열거(표시712와 불일치). 개별 읽기·요약은 별도 배치 ledger로 추적. | 전체 블로그2116개와 구분. 일반 공개 접근만 사용; 이미지/외부링크·주장 검증은 별도 상태. |
| U09 | [재테크 입문자가 경제 기사 읽기 보다 먼저 해야할 일 - 숫자에 대한 감 #1](naver-sources.md) | 지정 글 본문 텍스트 확인. U08의 inventory index264와 동일 글로 중복 합산하지 않음. | 이미지·외부 기사와 당시 수치의 독립 검증은 미수행. |
| U10 | [Empirical Asset Pricing via Machine Learning](papers.md) | 만료 가능 토큰 URL은 미보관. 저자 공개 게재본 초록·방법·데이터/분할·권리 고지 확인. | CC BY-NC-ND 4.0 표시. 전체 PDF 미러·Internet Appendix 전수 검토·코드/표 재현 없음. |
| U11 | [금융의 연금술 (한국어 전자책)](books.md) | 공식 상품정보·19장 목차·소개, 저자 공식 도서 안내와 별도 공개 글. DRM 본문0장. | 별도 저자 글을 책 본문으로 혼동하지 않음. 정식 접근과 재배포 허가는 별개. |
| U12 | [[퀀트논문 리뷰 #3] 오늘 주식이 올랐던 시간대에 내일도 오른다? Intraday Patterns in the Cross-section of Stock Returns](naver-sources.md) | 지정 글 본문 텍스트, Heston·Korajczyk·Sadka 원논문 서론·방법·관련 결론 대조. | 횡단면30분구간 연구와 단일 종목 방향 예측을 구분. 그림 픽셀·전표·실행 미검토. |

## 블로그 전수 여부를 판단하는 법

- **nomore_bet:** [전체714개 목록](naver-nomore-inventory.md)에 대한 개별 본문·요약 상태를 맞춘다. 표시712/열거714 불일치는 미해결이다. U09는 목록264번과 같은 글이다. [개별 배치](naver-nomore-post-notes/)의 기록이 생겼다고 전체가 완료된 것은 아니다.
- 이 명세를 생성할 때 개별 배치에 존재한 unique 글은 **230개**다. 진행 중이므로 최신 배치·통합 ledger를 우선한다.
- **dnpquant:** 공개웹0건과 사용자 업로드175파일을 별도 분모로 둔다. 175개파일을 모두 확인했으며173개식별본문과2개오류파일을 구분한다. 결합본·부분 발췌는 [archive coverage](investor-dnpquant/coverage.md)에서 확인한다. 공개블로그 전체를 확보했다고 표현하지 않는다.

## 접근 불가 및 권리 경계

- ***REMOVED*** 지정 공고는404여서 제목도 추측하지 않았다. 보관본 또는 새 공식 링크가 필요하다.
- 책의 구독/DRM 본문은 읽지 않았다. 공식 공개 미리보기·목차·초록만 요약했으며 정식 접근이 있더라도 책 대체본을 재게시하지 않는다.
- Silverchair 인증·워터마크 URL은 저장하지 않고 DOI·저자 공개본으로 대체했다.
- Google Calendar 링크(U13)는 개인 일정이며 연구자료 수집에서 제외했다. URL·일정 내용은 명세에 보관하지 않는다.
- repo에는 짧은 독자적 해설·출처·검토 상태만 둔다. 원문 전체·이미지·데이터를 반입하려면 항목별 권리를 별도로 확인한다.

## 후속 완료 판정

1. 접근 가능한 블로그 모든 unique ID에 읽기/요약/비연구/차단 등 정직한 상태가 있어야 한다. 미읽음을 제목 기반 요약으로 대체하지 않는다.
2. mlfactor19장 본문은 검토했지만 링크된15개Python자료와 참고문헌 전체는 별도 범위다.
3. 논문의 주요 절 독서와 전표/부록 재현은 다른 작업이다. 재현·거래 가능한 알파 검증을 했다고 표시하지 않는다.
4. 접근 불가·권리 제한 때문에 남는 범위는 명시하며, 해소 전까지 “전부 완료”라고 말하지 않는다.
