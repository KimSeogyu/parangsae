# 사용자 지정 논문 — 정본 추적 및 검증 설계

확인일: 2026-10-23. 원문 전체를 저장소에 복사하지 않고, 실제 확인한 범위와 짧은 독자적 노트를 제공한다. 아래 제안은 논문의 실증 결과와 별개인 개발 판단이다.

## U03. Value and Momentum Everywhere

- 사용자 링크: [SSRN 2174501](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2174501)
- 저자: Clifford S. Asness, Tobias J. Moskowitz, Lasse Heje Pedersen.
- 버전: 사용자 링크는 2012년 작업논문. 최종판은 *Journal of Finance* 68(3), 929–985, 2013, DOI [10.1111/jofi.12021](https://doi.org/10.1111/jofi.12021).
- 접근: SSRN은 403. [저자 대학 홈페이지 최종판 PDF](https://pages.stern.nyu.edu/~lpederse/papers/ValMomEverywhere.pdf)는 정상. 게재본 57쪽의 본문·각주·표 텍스트·참고문헌 목록과 6개 그림(9쪽)을 검토했다. [CBS 저자수락본](https://research-api.cbs.dk/ws/files/44523827/lasse_heje_pedersen_value_and_momentum_postprint.pdf)의 별도 부록 10쪽도 텍스트로 읽었다. 판본·매체별 범위는 [독서 ledger](paper-u03-full-review.md) 참조. 계산 재현은 하지 않았다.

**내용 요약:** 8개 시장에서 자산별 가치와 횡단면 모멘텀을 함께 분석한다. 모멘텀은 최근 한 달을 제외한 과거 12개월 수익이며 3분위·순위가중 롱숏 및 50:50 결합을 비교한다. 같은 스타일은 시장 간 공통 움직임을, 가치와 모멘텀은 음의 상관을 보인다. 결과는 **비용 차감 전**이며 모든 개별 프리미엄이 유의하지는 않다. 유동성은 부분적 설명이고 제안한 3요인 모형도 GRS 검정을 통과하지 못한다. [§I–VI](https://pages.stern.nyu.edu/~lpederse/papers/ValMomEverywhere.pdf)

**데이터 경로:** [원논문 데이터](https://www.aqr.com/insights/datasets/value-and-momentum-everywhere-original-paper-data)는 1972-01~2011-07 자료로 설명된다. [업데이트 요인](https://www.aqr.com/Insights/Datasets/Value-and-Momentum-Everywhere-Factors-Monthly)과 [롱온리 포트폴리오](https://www.aqr.com/Insights/Datasets/Value-and-Momentum-Everywhere-Portfolios-Monthly)는 다른 버전이다. 파일 자체 다운로드·검산은 하지 않았다.

**개발 제안·질문(추론):** 주식 가치의 코인 대입과 달러중립의 베타중립 취급을 피한다. 본문의 자산군 결합은 **전체 표본 변동성**을 쓴다(pp.945–946, 각주11·13, §V.D pp.977–978). 과거 데이터만으로 변동성을 추정하고 수익 구간 전에 가중치를 고정해야 한다. 저자는 과거 3년 일수익 기반 대안도 설명하므로 연구용 사후 결과와 구현 사양을 분리한다. 비용·펀딩·차입·롤·유동성 및 원논문/최신 확장 실험을 구분했는가? 50:50은 최적값 보장이 아니다.

**권리·남은 범위:** 공개 열람은 재배포 허가가 아니다. PDF·데이터는 저장소에 반입하지 않았다. 별도 부록은 2012-06 표기 저자수락본의 첨부 텍스트이며 게재판 부록과 수치 동일성을 보장하지 않는다. 부록 그림 픽셀, 원 데이터·코드·표 재현은 미검토다.

## U10. Empirical Asset Pricing via Machine Learning

- 사용자 입력은 만료 가능한 Silverchair PDF 링크였다. 인증·워터마크 토큰은 이 문서 및 저장소에 보관하지 않는다.
- 정본: Shihao Gu, Bryan Kelly, Dacheng Xiu, *Review of Financial Studies* 33(5), 2223–2273, 2020. DOI [10.1093/rfs/hhaa009](https://doi.org/10.1093/rfs/hhaa009), [출판사 페이지](https://academic.oup.com/rfs/article/33/5/2223/5758276).
- 접근: 출판사 HTML은 도구 리디렉션 실패. [저자 대학의 게재본 51쪽](https://dachxiu.chicagobooth.edu/download/ML.pdf)과 [공식 Internet Appendix 31쪽](https://dachxiu.chicagobooth.edu/download/ML_supp.pdf) 전체 텍스트를 읽었다. 모든 표·그림의 구조·축·캡션을 추가 대조했으며 [페이지별 독서 원장과 인쇄식 주의](paper-u10-full-review.md)를 제공한다. [2019-09-13 작업본](https://dachxiu.chicagobooth.edu/download/ML_BKP.pdf)은 별도 판본이다.

**내용 요약:** 미국 주식 월간 위험프리미엄을 선형·정규화·차원축소·트리·신경망으로 비교한다. 1957–2016년 표본에 종목 특성 94개와 거시변수 8개 등을 이용한다. 비선형 상호작용과 모멘텀·유동성·변동성의 중요성을 보고한다. 초기 학습/검증/시험은 1957–1974/1975–1986/1987–2016이며 매년 재학습한다. 분기/연간 공시에는 최소 4/6개월 지연을 적용한다. [§1–2 및 각주 30·32](https://dachxiu.chicagobooth.edu/download/ML.pdf)

**개발 제안·질문(추론):** 선형·0예측·모멘텀 기준선을 두고 학습·튜닝·최종시험을 분리한다. 공시의 실제 배포시각을 검증한다. 크립토·장중·비용 후 수익으로 성과를 외삽하지 않는다. 추가 지표 차이와 인쇄식 검사는 위 독서 원장에만 기록한다.

**권리·남은 범위:** 게재본 첫 페이지는 CC BY-NC-ND 4.0을 명시한다. 짧은 독자적 해설과 링크만 제공하며 PDF를 복제하지 않았다. 본문·부록 독해는 완료했지만 원 데이터·저자 코드 열람/실행·표 재현·인용 논문 전수 독해·모든 수식의 독립 증명은 하지 않았다. 부록의 독립 재배포 허가는 확정하지 않았다.
