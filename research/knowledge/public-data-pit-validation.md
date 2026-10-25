# 공개 뉴스·대체데이터의 PIT 검증 보완

확인일: 2026-10-23 UTC. 시간당 리서처의 자료 검토이며 collector 실행·시장자료 분석은 아니다. BTCUSDT 1분~4시간 bar 연구에 적용할 **검증 계약 제안**을 정리한다. 기존 코드가 아래 필드를 사용한다고 확인하지 않았으므로 구현 결함으로 보고하지 않는다. 새 알파·전략 채택·동결 sweep/holdout 변경 제안도 아니다.

핵심은 **현재 내려받을 수 있는 과거 데이터와 당시 관측 가능했던 데이터가 다를 수 있다**는 점이다. [공통 이벤트 스키마와 검증 규약](alternative-signals.md#4-모든-후보에-적용할-검증-규약)을 다음 세 사례로 구체화한다.

후속 [Funding·검색 관심도 경계](funding-attention-source-boundaries.md)는 현재 metadata와 과거 regime, Google Trends 제품별 접근·시점 차이를 별도 coverage와 함께 다룬다. 아래 세 사례의 기존 확인일·한계는 그대로 유지한다.

2026-10-25 후속 [장애 자료의 출처/PIT 경계](outage-source-boundaries.md)는 거래소 사건 후보의 별도 입력 계약과 coverage다. 기존 세 사례의 검토일·한계를 변경하지 않는다.

## 1. Wikimedia: 공개 역사도 봇 재분류로 소급 수정된다

**공식 운영자 설명:** [데이터 품질 사건 문서의 고정 revision 2462581][W01]은 다음을 기록한다.

| 구분 | 문서에 기록된 시점 |
|---|---|
| 이상 트래픽 시작 / 탐지 | 2025년 11월 / 2025년 12월 |
| 새 봇 분류 규칙 배포 | 2026-04-04 |
| 공개 pageview·unique-device 역사 정정 작업 | 2026-04-28 시작, 2026-05-15 완료 |
| 정정 가능한 역사 시작 | 2025-12-01 |
| 정정하지 못한 기간 | 2025년 11월: 필요한 원자료가 보존정책에 따라 삭제됨 |

공식 설명은 봇이 `user`로 오분류되었고 per-article를 포함한 pageview 계열이 영향을 받았다고 한다. 고정 revision의 최종 편집 표시는 **2026-10-01 09:42**이다. 문서 제목의 `2026-06-10`은 이번 검토에서 확인한 최초 게시시각이 아니며, 사건·정정·문서 편집 시각을 서로 대체하지 않는다.

**독립 확인 범위:** 위 공개 운영 문서와 revision을 읽었다. 원자료 재계산, Bitcoin 페이지별 영향, BTC 가격·성과 영향은 확인하지 않았다. 연결된 작업 티켓 본문은 접근 실패로 미검토다.

**검증 제안:** `agent=user`를 영구적인 인간 방문 확정값으로 취급하지 않는다. 응답 vintage·분류 정의·수신시각을 보존하고, 나중 정정본으로 과거 입력을 덮어쓰지 않는다. 나중 공개된 품질 경고도 당시 알려진 특징으로 소급 주입하지 않는다. 이번 검토는 품질 메타데이터에 한정하며, 잠긴 시장 holdout 열람이나 성과를 보고 한 기간 제외는 하지 않았다.

## 2. GDELT GAL: 기사 시각·발견 시각·backfile 날짜는 다르다

**공식 문서 설명:** [2021-12-06 GAL 발표][G01]의 `date`는 기사 게시시각이 있으면 그 값, 없으면 GDELT가 본 시각이다. 하나의 필드가 두 의미를 갖는다. 문서는 첫 backfile을 `20200101000100.gal.json.gz`로 제시하고 역사 재구성 중 중복을 경고한다. 이 파일명은 GAL 서비스가 2020년에 이미 공개되었다는 증거가 아니다.

같은 발표는 분 단위 JSON 파일이 당시 15분 갱신 뒤 몰릴 수 있고, RSS가 매분 갱신되는 **rolling 15분 발견 창**이라고 설명한다. 이는 **2021년 문서의 제품 설명**이다. 2026-10-23 실제 feed/backfile 응답·완전성·현재 창 길이·배포 지연은 검증하지 못했다. GAL 설명을 DOC/Event/GKG 전체에 일반화하지 않는다.

**검증 제안:** 원문 게시시각과 출처, GDELT 발견시각의 확인 수준, 파일 수신시각을 분리한다. `date`만으로 최초 도착순서를 복원하지 않는다. 중복 URL과 내용 revision을 구분하고, backfill은 수신 당시부터만 사용한다. **시간당 연구 검토 주기와 실제 collector 수집 주기는 별개**다. 이 문서는 collector를 가동하지 않았고, 현재 rolling 창이 유지되는지 실측하기 전 시간당 polling의 완전성을 주장하지 않는다.

## 3. Bitcoin/mempool: 시계·관측 모집단·재동기화를 분리한다

- [Bitcoin 블록 헤더 문서][B01]의 `time`은 채굴자가 헤더에 넣는 시각이다. 로컬 노드나 연구 시스템의 최초 수신시각 대신 쓰지 않는다.
- [mempool FAQ][M01]는 small-node purge 기준을 기본 300 MB 노드에서, 화면의 memory usage를 더 큰 한도의 노드에서 가져온다고 설명한다. 두 표시값을 같은 모집단의 분자·분모로 가정하지 않는다. 확인은 공식 FAQ의 검색 색인 본문이며, 실시간 노드 정책·값은 독립 측정하지 않았다.
- [upstream #3221][M02]은 2023-03-04 캐시 재구축 때 잘못된 빈 mempool 표시를 보고했다. [2024-03-25 후속 의견][M03]은 Redis 기반 production에서는 괜찮다고 생각하나 community disk-cache 환경에서는 남을 수 있다는 조건부 판단이다. **현재 hosted production 버그의 재현 증거가 아니다.**

**검증 제안:** 공급자/노드 정책·집계 범위·실제 관측시각·재시작/재동기화 상태를 구분한다. 빈 값·연결 실패·불완전 캐시를 경제적 수요 0으로 자동 변환하지 않는다. 조건을 알 수 없으면 `unknown`으로 남긴다. 과거 first-seen 재구성 가능성, hosted 데이터 재사용 권리와 연속 수집 비용은 아직 운영 채택 조건을 충족했다고 확인하지 못했다.

## 4. 개발자가 채택 전에 확인할 계약

아래는 **실행하지 않은 acceptance checks**다. 기존 `EventTs` / `AvailableTs` / `ReceivedTs` 및 `Causal<T>` 의미를 바꾸지 않고, 담당 개발자가 기존 구조와 맞춰 검토한다.

| 항목 | 제안 계약 / 기각·보류 조건 |
|---|---|
| 시각 계보 | `event_time`, `source_claimed_time`, `first_received_at`, `revision_received_at`을 구별. 관측기간/파일명만 있으면 실제 공개·수신시각을 추정값으로 채우지 않는다. |
| 버전 계보 | `source_product`, `metric_definition_version`, `payload_hash`, `is_backfill` 보존. 동일 payload 재수신은 중복, 변경본은 새 revision으로 구별한다. |
| 모집단·결측 | `observation_population`, 해당 시 `node_policy_version`, `capture_gap`, `restart_or_resync_flag`를 기록. 알 수 없는 상태와 정상 0을 분리한다. |
| 늦은 자료 | 늦게 수신한 backfill·정정본이 더 이른 의사결정 입력으로 들어가면 PIT 적합 판정 불가. 원래 관측본도 보존한다. |
| 재현 한계 | 최종 역사만 확보되면 pseudo-real-time이라고 표시. 검증된 당시 이용 가능성 없이 실시간 수익의 증거로 승격하지 않는다. |

데이터 계약을 통과해도 예측력은 별개다. 사전 고정한 가격·거래량·시간대 기준모형, 사건 중복/다중검정, 시간순 OOS, 관측 지연 및 수수료·스프레드·슬리피지·펀딩을 포함한 비교가 필요하다. 기준모형 대비 증분효과가 없거나 현실 지연·비용에서 사라지면 거래 가설을 채택하지 않는다. 여기서는 수익률·Sharpe·백테스트·collector 코드를 만들거나 실행하지 않았다.

## 5. 이번 보완의 source coverage와 접근 경계

| ID | 직접 읽은 범위 | 남은 한계 |
|---|---|---|
| W01 | Wikimedia 공식 사건 문서와 revision 식별자·편집 표시 | 수치 재현·최초 게시시각·Bitcoin별 영향 미검증; 연결 티켓 미열람 |
| W02 | [Analytics API 접근 정책][W02] | API 데이터 CC0 1.0과 문서 텍스트의 권리를 구분; 현재 호출 정책 준수 필요 |
| G01 | GAL 공식 발표의 필드·backfile·RSS 설명 | 현재 feed/역사 파일 직접 응답 및 SLA 미검증; 원기사 재배포 권리를 뜻하지 않음 |
| B01 | Bitcoin 개발자 헤더 정의 | 실제 노드 수신시각/지연 미측정 |
| M01 | 공식 FAQ의 검색 색인 설명 | 직접 페이지 텍스트 추출은 비어 있었음; 현행 backend 모집단 미측정 |
| M02/M03 | upstream 보고와 후속 댓글 원문·작성일 | 배포별 재현·현재 production 영향 미검증 |

이 보완은 공개 자료의 짧은 독자적 요약과 검증 제안만 포함한다. 공개 열람은 무제한 자동수집·원기사/hosted 데이터 재배포 허가가 아니다. 신규 계정·키·결제, 개인 방문자/지갑 추적, 군사 활동 추론은 하지 않는다.

[W01]: https://wikitech.wikimedia.org/w/index.php?title=Data_Platform/Data_Lake/Data_Issues/2026-06-10_Nov_2025_spike_in_bot_traffic&oldid=2462581
[W02]: https://doc.wikimedia.org/generated-data-platform/aqs/analytics-api/documentation/access-policy.html
[G01]: https://blog.gdeltproject.org/announcing-the-gdelt-article-list-rss-feed/
[B01]: https://developer.bitcoin.org/reference/block_chain.html
[M01]: https://mempool.space/docs/faq
[M02]: https://github.com/mempool/mempool/issues/3221
[M03]: https://github.com/mempool/mempool/issues/3221#issuecomment-2017399069
