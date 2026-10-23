# MLFactor Python 노트북 정적 검토

**공식 15개 링크를 모두 개별 검토했다. 노트북 코드 실행·학습·예제 데이터 다운로드는 0건이다.** 이 문서는 기존 `parangsae` 개발자가 예제를 검증 가능한 연구 설계로 옮길 때 볼 독서 기록이다. 아래 독립 검토는 원저자의 설명과 구분되며, 현재 저장소의 결함을 판정하지 않는다.

- 원저자: Guillaume Coqueret, Tony Guida. 확인일: 2026-10-23.
- 대상: [공식 18장 Python 색인](https://www.mlfactor.com/python.html)에 연결된 **초판 Python 판** 1–8장·10–16장. 9장 Python 링크는 원래 색인에 없다. 별도 Google Drive 구현과 외부 참고문헌은 범위 밖이다.
- 교재의 [19개 장 독서 기록](mlfactor.md)과 이 **15개 노트북 정적 검토**를 구분한다. 원본 `.ipynb`를 확보하거나 실행한 것이 아니라 공식 게시 HTML을 읽었다.
- 검토량: 렌더링 셀 **1,838개** = markdown **1,665개** + 코드 **173개**(빈 코드 셀 3개 포함). 게시 출력 블록 **150개**의 이용 가능한 정적 텍스트·표 및 그림 배치 **76건**을 확인했다.
- 한계: 13장 LIME·DALEX의 JavaScript 의존 도표 2개는 렌더링하지 않았다. 그 코드와 남아 있는 텍스트는 읽었으며 SHAP의 정적 그림은 확인했다. 게시 수치는 원저자의 과거 출력이며 재현·성능·최신 라이브러리 호환성 검증이 아니다.
- 재사용: 공개 열람은 코드·데이터 재배포 허락과 다르다. 이 자료에는 독자적 짧은 메모와 출처만 두고 원문 코드·HTML·그림·데이터는 포함하지 않는다.

## 커버리지와 출처

셀 번호는 **렌더링 HTML 문서순 1기반**이다. 원저자의 `In[...]` 실행번호나 원본 노트북 식별자가 아니다. 1·2장은 진입 페이지의 공식 `object`가 가리키는 실제 자료까지 읽었으며, [JSON manifest](mlfactor-python-notebooks.json)에 실제 문서와 wrapper의 URL·해시를 분리했다. 해시는 공개 HTML 응답 바이트 기준이고 출처의 영구 불변성을 뜻하지 않는다.

| 장 | 공식 진입점 | 전체 셀 | Markdown | 코드 | 출력 블록 | 검토 그림 | 상태 |
|---|---|---:|---:|---:|---:|---:|---|
| 1 | [Notations and data](https://www.mlfactor.com/chap_1.html) | 36 | 27 | 9 | 5 | 2 | 정적 완료 |
| 2 | [Introduction](https://www.mlfactor.com/chap_2.html) | 36 | 35 | 1 | 0 | 1 | 정적 완료 |
| 3 | [Factor investing and asset pricing anomalies](https://www.mlfactor.com/chap_3.html) | 169 | 157 | 12 | 10 | 4 | 정적 완료 |
| 4 | [Data preprocessing](https://www.mlfactor.com/chap_4.html) | 143 | 134 | 9 | 9 | 7 | 정적 완료 |
| 5 | [Penalized regressions and sparse hedging for minimum variance portfolios](https://www.mlfactor.com/chap_5.html) | 115 | 105 | 10 | 5 | 3 | 정적 완료 |
| 6 | [Tree-based methods](https://www.mlfactor.com/chap_6.html) | 135 | 113 | 22 | 16 | 6 | 정적 완료 |
| 7 | [Neural networks](https://www.mlfactor.com/chap_7.html) | 246 | 226 | 20 | 13 | 15 | 정적 완료 |
| 8 | [Support vector machines](https://www.mlfactor.com/chap_8.html) | 49 | 46 | 3 | 3 | 4 | 정적 완료 |
| 10 | [Validating and tuning](https://www.mlfactor.com/chap_10.html) | 150 | 135 | 15 | 14 | 10 | 정적 완료 |
| 11 | [Ensemble models](https://www.mlfactor.com/chap_11.html) | 92 | 71 | 21 | 17 | 3 | 정적 완료 |
| 12 | [Portfolio backtesting](https://www.mlfactor.com/chap_12.html) | 174 | 159 | 15 | 9 | 6 | 정적 완료 |
| 13 | [Interpretability](https://www.mlfactor.com/chap_13.html) | 121 | 109 | 12 | 19 | 6 | 정적 완료; JS 도표 2건 제외 |
| 14 | [Two key concepts: causality and non-stationarity](https://www.mlfactor.com/chap_14.html) | 126 | 120 | 6 | 9 | 5 | 정적 완료 |
| 15 | [Unsupervised learning](https://www.mlfactor.com/chap_15.html) | 116 | 102 | 14 | 17 | 3 | 정적 완료 |
| 16 | [Reinforcement learning](https://www.mlfactor.com/chap_16.html) | 130 | 126 | 4 | 4 | 1 | 정적 완료 |

**1·2장 실제 내용:** [CHAPTER_1](https://www.mlfactor.com/material/CHAPTER_1.html), [CHAPTER_2](https://www.mlfactor.com/material/CHAPTER_2.html). 나머지는 진입 URL 자체가 노트북 HTML이다. 그림 수는 중복 여부를 합친 고유 자산 수가 아니라 각 문서의 `<img>` 배치 수다.

## 개별 읽기 메모

### 1 Notations and data

**내용:** 종목·시점 패널, 선행 수익률 라벨, 날짜 분할과 횡단면 상대성과 분류를 준비한다. 공개 표와 그림은 예제 데이터의 구조를 보여준다.

**독립 개발 검토:** 93개 특성 설명과 92열 위치 슬라이스의 차이를 확인하라. 완전관측 종목의 사후 선별은 생존편향을 만들 수 있다. 라벨 가용시각·동률·인덱스 정렬을 검사하라.

**근거:** 셀 16, 23, 28, 31, 33. [공식 노트북](https://www.mlfactor.com/material/CHAPTER_1.html)

### 2 Introduction

**내용:** 데이터 처리에서 예측·가중치·성과평가로 이어지는 연구 흐름을 설명한다. 알고리즘 복잡성보다 경제적 가설과 입력 품질, 반복 검증을 강조하며 실행 내용 없는 빈 코드 셀을 포함한다.

**독립 개발 검토:** 흐름도를 자동매매 구현 완료로 오인하지 말라. 예측 정확도와 비용 후 포트폴리오 성과를 분리하고, 모델 선택과 최종 평가 구간을 고정하라.

**근거:** 셀 8, 10, 16, 18, 22, 36. [공식 노트북](https://www.mlfactor.com/material/CHAPTER_2.html)

### 3 Factor investing and asset pricing anomalies

**내용:** 규모 정렬 포트폴리오, Fama–MacBeth 회귀, 팩터 중복 검정과 자기상관을 예시한다. 팩터 발견의 다중검정·출판편향·재현성 한계도 다룬다.

**독립 개발 검토:** ZIP 해제·날짜열 생성이 제시 코드에 없다. 고정 227행과 수익률 shift의 날짜 정렬을 검증하라. 전체표본 베타를 과거 운용 신호로 재사용하면 미래정보 위험이 있다.

**근거:** 셀 34, 68, 76, 89, 91. [공식 노트북](https://www.mlfactor.com/chap_3.html)

### 4 Data preprocessing

**내용:** 결측·이상치·횡단면 정규화, 범주·삼중장벽 라벨과 예측기간을 다룬다. 합성 데이터의 변환 방식에 따라 회귀 결과가 달라지는 예를 제시한다.

**독립 개발 검토:** 정규화 함수에 상수열·결측 방어가 없다. 자기상관 계산 전 종목별 날짜 정렬을 보장하라. 설명용 전체표본 변환을 실전 학습에 복사하지 말고 시점별 처리·라벨 겹침을 검증하라.

**근거:** 셀 18, 23, 24, 49, 120, 126. [공식 노트북](https://www.mlfactor.com/chap_4.html)

### 5 Penalized regressions and sparse hedging for minimum variance portfolios

**내용:** Lasso·Ridge·ElasticNet 계수 경로, 희소 헤지 최소분산 배분, 표본외 예측오차와 방향 적중률을 비교한다.

**독립 개발 검토:** 게시 표는 MV 변동성 0.03350, Sparse 0.03474로 인접 우위 서술과 맞지 않는다. 수축·정규화 분모, 고정 매개변수와 무비용 배분을 확인하고 테스트셋 튜닝을 별도 검증셋으로 분리하라.

**근거:** 셀 38, 89, 91, 93, 94, 113. [공식 노트북](https://www.mlfactor.com/chap_5.html)

### 6 Tree-based methods

**내용:** 의사결정나무에서 랜덤포레스트·AdaBoost·XGBoost로 확장하고 단조성, 표본·변수 축소, 인스턴스 가중치를 보여준다.

**독립 개발 검토:** 셀110의 100분위 정수 코드와 0.2/0.8 임계값이 불일치한다(관찰). 결측을 제외한 전 구간 선택으로 꼬리 40% 추출 의도가 깨지는 위험이다(정적 추론). 셀114·121 공개 출력에는 미사용 가능 매개변수 경고도 있다.

**근거:** 셀 28, 54, 84, 110, 114, 121, 127. [공식 노트북](https://www.mlfactor.com/chap_6.html)

### 7 Neural networks

**내용:** 퍼셉트론·역전파부터 Keras 회귀·분류, 사용자 손실, GRU까지 연결하고 GAN·CNN·오토인코더 구조를 소개한다.

**독립 개발 검토:** 셀118·122·131·161의 예측/라벨 형상은 (n,1)/(n,)이다(코드 관찰): 행별 적중률 대신 n×n 브로드캐스팅 위험이 있다(정적 추론). 셀137·144의 softmax와 로짓 손실 설정 불일치는 셀147의 공개 경고에도 드러난다.

**근거:** 셀 118, 122, 131, 137, 144, 147, 161, 187, 196. [공식 노트북](https://www.mlfactor.com/chap_7.html)

### 8 Support vector machines

**내용:** 마진·슬랙·커널을 설명하고 제한된 표본으로 SVM 회귀·분류의 공개 평가 결과를 비교한다.

**독립 개발 검토:** 셀40·43은 앞 장의 XGBoost 변수·시험 라벨을 재사용하며 첫 1,000행만 선택한다(관찰). 독립 실행에 필요한 초기화가 없고 표본 순서에 민감할 수 있다(정적 추론); 평가 수치는 재현 검증하지 않았다.

**근거:** 셀 39, 40, 41, 43, 44. [공식 노트북](https://www.mlfactor.com/chap_8.html)

### 10 Validating and tuning

**내용:** 회귀·분류 평가, 편향–분산, 과적합 억제와 격자·베이지안 탐색 및 시계열 검증을 정리한다.

**독립 개발 검토:** 셀38은 ROC에 확률 대신 분류 결과를 입력한다(관찰): 순위 정보가 소실된다(정적 추론). 셀124·133의 2분할 교차검증에는 시간순 분할 지정이 없어 미래정보 누출 여부를 확인해야 한다.

**근거:** 셀 38, 69, 124, 133, 140, 141. [공식 노트북](https://www.mlfactor.com/chap_10.html)

### 11 Ensemble models

**내용:** 오차 상관과 가중치 제약을 중심으로 선형·스태킹 앙상블을 비교하고 거시조건 및 연도별 학습표본 분리를 살핀다.

**독립 개발 검토:** 셀25는 신경망 오차에 기본 행번호를 붙여 기존 인덱스의 오차들과 결합한다(관찰): 정렬 불일치·결측 오염 위험이다(정적 추론). 셀57은 학습내 예측으로 스태킹하므로 별도 OOF 예측 검증이 필요하다.

**근거:** 셀 25, 30, 33, 45, 57, 65, 76, 86. [공식 노트북](https://www.mlfactor.com/chap_11.html)

### 12 Portfolio backtesting

**내용:** 롤링 학습과 라벨 유예기간, 신호의 포트폴리오 가중치 변환, 거래비용·성과지표를 연결한다. XGBoost와 동일가중 비교 및 다중 전략 탐색의 과최적화를 다룬다.

**독립 개발 검토:** 관찰: 셀130·138은 Tt행을 만들고 Tt−1행만 채우며, 147은 전체를 평가한다. 추론: 마지막 0행이 성과·회전율을 왜곡할 수 있다. 셀133에는 예측이 모두 동률일 때 가중치 합계 0을 방어하는 분기가 없다.

**근거:** 셀 130, 133, 138, 147. [공식 노트북](https://www.mlfactor.com/chap_12.html)

### 13 Interpretability

**내용:** 대리 트리, 변수 중요도, 부분의존도와 LIME·SHAP·Breakdown을 전역·국소 해석으로 구분한다. 기존 예측모델의 설명을 비교하되 해석 범위와 표시상의 제약을 드러낸다.

**독립 개발 검토:** 관찰: 셀44는 순열마다 Ridge를 재학습하고 같은 데이터에서 손실을 잰다. 추론: 설명한 고정모델 순열 중요도에 재적합 효과가 섞인다. 셀31은 선택한 세 변수만 재정규화하므로 전체 중요도로 읽으면 안 된다.

**근거:** 셀 31, 33, 40, 42, 44, 83, 115. [공식 노트북](https://www.mlfactor.com/chap_13.html)

**범위 한계:** JavaScript 의존 LIME(셀83)·DALEX(셀115) 도표는 미렌더링. SHAP(셀100) 정적 PNG는 확인.

### 14 Two key concepts: causality and non-stationarity

**내용:** Granger 검정, 인과그래프, 구조적 시계열을 통해 예측과 인과를 구분한다. 연도별 수익·계수 변화에서 비정상성을 살피고 온라인 학습과 전이학습의 적응 원리를 설명한다.

**독립 개발 검토:** 관찰: 셀45는 반환된 graph_pc 대신 model_pc 추정기를 그리기에 전달한다. 원자료가 아닌 상관행렬도 학습 입력으로 넘긴다. 추론: 게시 그래프의 재현성과 인과해석 전에 객체 및 입력 의미를 검증해야 한다.

**근거:** 셀 17, 42, 45, 55, 71, 75. [공식 노트북](https://www.mlfactor.com/chap_14.html)

### 15 Unsupervised learning

**내용:** 다중공선성을 출발점으로 PCA·오토인코더의 압축과 k-means의 변수 군집화를 비교한다. 최근접 이웃 예제로 거리 가중 예측과 시점 누출의 한계를 보여준다.

**독립 개발 검토:** 관찰: 셀103은 대상 종목을 제외한 표본에 이웃을 맞추지만, 셀106은 제외 전 표본에서 위치 인덱스로 수익률을 찾는다. 추론: 라벨 정렬이 어긋날 수 있다. 셀48은 PCA 학습평균을 빼지 않고 투영한다.

**근거:** 셀 41, 48, 103, 104, 106, 108. [공식 노트북](https://www.mlfactor.com/chap_15.html)

### 16 Reinforcement learning

**내용:** MDP, Q-learning, SARSA와 정책경사를 설명하고 모의 수익률 및 두 종목 예제를 연결한다. 상태·행동 차원의 폭증과 희소한 금융 표본이 정책 추정에 주는 제약을 강조한다.

**독립 개발 검토:** 관찰: 셀102는 상태와 보상을 같은 수익률로 만든다. 셀104·113은 갱신 목표에서도 무작위 행동을 선택한다. 추론: 미래 보상 시점과 표준 Q-learning의 최대값 갱신에 맞지 않아, 제시된 정책을 예측성과로 읽으면 안 된다.

**근거:** 셀 36, 99, 102, 104, 110, 113. [공식 노트북](https://www.mlfactor.com/chap_16.html)

## 개발자가 먼저 확인할 항목

- **시점·표본:** 1·3·10·12·16장의 라벨 시점과 학습·평가 구분.
- **배열·키·분모:** 1·4·7·11·12·15장의 행 정렬, 형상, 빈 선택·상수열.
- **실험의 의미:** 5·6·13·14장의 설명·실제 계산·게시 출력 일치.
- 이 목록은 후속 검증 제안이다. 데이터 도입·학습·백테스트·거래 구현을 승인하거나 수행한 기록이 아니다.
