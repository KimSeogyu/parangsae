# 사용자 지정 논문 — 정본 추적 및 검증 설계

확인일: 2026-10-23. 원문 전체를 저장소에 복사하지 않고, 실제 확인한 주요 절과 짧은 독자적 노트를 제공한다. 아래 제안은 논문의 실증 결과와 별개인 개발 판단이다.

## U03. Value and Momentum Everywhere

- 사용자 링크: [SSRN 2174501](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2174501)
- 저자: Clifford S. Asness, Tobias J. Moskowitz, Lasse Heje Pedersen.
- 버전: 사용자 링크는 2012년 작업논문. 최종판은 *Journal of Finance* 68(3), 929–985, 2013, DOI [10.1111/jofi.12021](https://doi.org/10.1111/jofi.12021).
- 접근: SSRN은 403. [저자 대학 홈페이지 최종판 PDF](https://pages.stern.nyu.edu/~lpederse/papers/ValMomEverywhere.pdf)는 정상. 초록, 데이터·신호 정의, 포트폴리오 구성, 비용·공매도·강건성 절을 선별 검토했다. 모든 표/부록을 재현하지 않았다.

**내용 요약:** 4개 지역 주식, 주가지수 선물, 국채, 통화, 상품선물에서 가치와 모멘텀을 함께 분석한다. 같은 스타일은 시장 간 공통 움직임을 보이며, 가치와 모멘텀끼리는 음의 상관이 관찰된다. 모멘텀은 대체로 최근 한 달을 제외한 과거 12개월 수익이다. 가치는 자산별로 정의가 다르다. 3분위 포트폴리오와 순위기반 롱숏 요인을 만들며 50:50 조합도 비교한다. 주된 결과는 **비용 차감 전**이다. 이 연구가 모든 시장의 모든 신호가 유의하다고 보장하지 않으며, 국채 가치의 단순 정의에는 약한 결과도 있다. [논문 I·II·V절](https://pages.stern.nyu.edu/~lpederse/papers/ValMomEverywhere.pdf)

**데이터 경로:** [원논문 데이터](https://www.aqr.com/insights/datasets/value-and-momentum-everywhere-original-paper-data)는 1972-01~2011-07 자료로 설명된다. [업데이트 요인](https://www.aqr.com/Insights/Datasets/Value-and-Momentum-Everywhere-Factors-Monthly)과 [롱온리 포트폴리오](https://www.aqr.com/Insights/Datasets/Value-and-Momentum-Everywhere-Portfolios-Monthly)는 다른 버전이다. 파일 자체 다운로드·검산은 하지 않았다.

**개발 제안·질문(추론):** 주식용 가치 정의를 코인에 억지 대응하지 말 것. 요인 노출 분산과 종목 수 분산을 구분할 것. 롱/숏·자산군별 비용, 펀딩, 차입, 롤 비용을 분리했는가? 원논문 재현과 최신 데이터 확장 실험을 분리했는가? 50:50 결합은 비교 기준이지 최적값이라는 주장이 아니다.

**권리·남은 범위:** 저자 공개본의 열람을 전체 재배포 허가로 보지 않는다. AQR 페이지에는 재배포 제한 안내가 있어 데이터/원문을 vendoring하지 않았다. 전체 표·추가 부록·데이터 검산은 후속 재현 항목이다.

## U10. Empirical Asset Pricing via Machine Learning

- 사용자 입력은 만료 가능한 Silverchair PDF 링크였다. 인증·워터마크 토큰은 이 문서 및 저장소에 보관하지 않는다.
- 정본: Shihao Gu, Bryan Kelly, Dacheng Xiu, *Review of Financial Studies* 33(5), 2223–2273, 2020. DOI [10.1093/rfs/hhaa009](https://doi.org/10.1093/rfs/hhaa009), [출판사 페이지](https://academic.oup.com/rfs/article/33/5/2223/5758276).
- 접근: 출판사 HTML은 도구 리디렉션 실패. [저자 대학의 게재본 PDF](https://dachxiu.chicagobooth.edu/download/ML.pdf)는 일반 공개 HTTP로 열람 성공. 초록, 방법, 데이터 구성, 학습·검증·시험 분할, 권리 고지를 확인했다. [2019-09-13 작업본](https://dachxiu.chicagobooth.edu/download/ML_BKP.pdf)도 확인했으나 정본과 혼동하지 않는다.

**내용 요약:** 미국 주식의 월간 위험프리미엄을 선형·정규화·차원축소·트리·신경망으로 비교한다. 1957–2016년 표본에 94개 종목 특성, 8개 거시변수, 업종 더미 등을 이용한다. 비선형 상호작용을 다루는 모델의 개선과 모멘텀·유동성·변동성 변수의 중요성을 보고한다. 초기 학습 1957–1974, 검증 1975–1986, 시험 1987–2016으로 시간순서를 유지하며 매년 재학습한다. 분기·연간 공시 특성에는 각각 최소 4·6개월 지연을 둔다. [게재본 §1–2, 특히 §2.1과 각주 30·32](https://dachxiu.chicagobooth.edu/download/ML.pdf)

**개발 제안·질문(추론):** 낮은 예측 오차와 비용 후 거래 효용은 별도 지표로 검사한다. 단순 선형/0예측/기본 모멘텀 기준선을 먼저 둔다. 학습·튜닝·최종시험 데이터가 겹치지 않는가? 공시 지연 대신 실제 배포시각을 쓸 수 있는가? 크립토·장중 데이터에서 재검증 없이 성과를 외삽하지 않는가? 논문만으로 거래비용 후 실현수익을 입증했다고 보고하지 말 것.

**권리·남은 범위:** 게재본 첫 페이지는 CC BY-NC-ND 4.0을 명시한다. 이 노트는 원문 번역/변형 배포가 아닌 짧은 독자적 해설과 링크다. 비상업·무변경 조건 충족을 보장할 수 없는 저장소에 PDF를 복제하지 않았다. Internet Appendix 전체, 원 데이터, 코드 실행 및 표 재현은 미수행이다.
