# 검색 인프라 및 채용 링크

확인일: 2026-10-23. 기술 설명·채용 요구조건과 투자수익의 실증 증거를 구분한다.

## U05. AWS OpenSearch 기술 글

- 정식 제목: 「Amazon OpenSearch Service의 검색 엔진으로서의 기능 설명」. Sewoong Kim, 2024-09-06. [검토 원문](https://aws.amazon.com/ko/blogs/tech/amazon-opensearch-services-vector-database-capabilities-explained/)
- 접근/범위: 공개 본문 전 구획을 확인했다. 코드나 이미지 원본을 재게시하지 않는다.

**내용 요약:** BM25·형태소 분석의 키워드 검색과 임베딩·k-NN 의미 검색을 대비한다. RAG, 근사/정확 검색, HNSW·IVF, 거리/점수, 필터, 하이브리드 검색, 행동자료 기반 LTR을 설명한다. 검색 품질·지연·메모리·필터링은 교환관계다. RAG용 검색이 된다는 사실은 문헌의 사실성이나 전략의 수익성을 보장하지 않는다. [AWS 글](https://aws.amazon.com/ko/blogs/tech/amazon-opensearch-services-vector-database-capabilities-explained/)

**현재 사양 확인:** 원문은 2024년 설명이다. 현재 공식 문서의 NMSLIB는 deprecated이며, 엔진·거리별 점수 변환도 서로 다르다. 모든 결과에 일괄 `1/(1+distance)`를 적용하는 구현은 피한다. Efficient filtering은 현재 Faiss IVF도 문서화한다. 배포할 관리형 서비스 버전에서 별도 확인해야 한다. [엔진](https://docs.opensearch.org/latest/mappings/supported-field-types/knn-methods-engines/), [거리/점수](https://docs.opensearch.org/latest/mappings/supported-field-types/knn-spaces/), [필터](https://docs.opensearch.org/latest/vector-search/filter-search-knn/index/)

**parangsae 적용 제안(추론):** 연구 검색 계층과 가격/주문 시뮬레이터를 분리한다. 각 문헌 청크에 source ID, 정본 URL, 발행일, 수집시각, 버전, 허용범위, 읽은 범위, 시장·주기 태그를 둔다. BM25 단독·벡터 단독·하이브리드를 동일 평가 질문으로 비교한다.

**검증 질문:** 원문 링크 없는 검색답변을 거부하는가? 동명 티커/한영 용어를 구분하는가? 미래 발행 문서가 과거 전략 신호에 섞이지 않는가? top-k·필터·ANN recall을 고정 정답셋으로 검사하는가? 수집 실패를 내용 부재로 오인하지 않는가?

## U06. 두나무 채용 공고 5734764004

- 참고 링크: [지정 공고](https://job-boards.greenhouse.io/dunamu/jobs/5734764004)
- 확인 결과: 원 URL 및 현재 Greenhouse board 주소에서 HTTP 404. 숫자 ID의 공개 검색에서도 해당 공고의 신뢰할 수 있는 본문·직무명을 확인하지 못했다. 공개 board API 경로는 웹 도구에서 열람되지 않았다.
- **제목·게시일·요구역량 미확인.** 추정 직무명이나 다른 공고 내용으로 대체하지 않는다. 삭제/만료/채용 시스템 이전 중 어느 원인인지 단정하지 않는다.
- 아직 추출한 내용이 없으므로 전략 근거·구현 요구사항으로 사용 금지. 합법적 보관본, 스크린샷, 새 공식 URL 중 하나를 받으면 이 ID와 대조한다.
- 개발상 구분(일반 제안): 채용공고는 조직 역량 참고자료일 뿐 팩터의 예측력 검증 자료가 아니다.
