/* ===========================================================
   EcoAlert · Tauri API 封装
   - 在 Tauri 环境下：调用 Rust 暴露的 command
   - 浏览器环境：降级到 mock（开发时可用 Vite 直接预览 UI）
   =========================================================== */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

const MOCK_PASSWORD = 'admin123';
const MOCK_KEY = 'ecoalert_mock_sources';
const MOCK_SOURCE_VERSION_KEY = 'ecoalert_mock_sources_version';
const MOCK_SOURCE_VERSION = 'local-video-v2';
const MOCK_GROUPS_KEY = 'ecoalert_mock_groups';
const MOCK_GROUP_VERSION_KEY = 'ecoalert_mock_groups_version';
const MOCK_GROUP_VERSION = 'local-video-v2';
const MOCK_HISTORY_KEY = 'ecoalert_mock_history';
const MOCK_HISTORY_VERSION_KEY = 'ecoalert_mock_history_version';
const MOCK_HISTORY_VERSION = 'real-detection-only-v1';
const MOCK_PW_KEY = 'ecoalert_mock_pw';
const MOCK_NTF_KEY = 'ecoalert_mock_notify_targets';
const MOCK_NTF_HISTORY_KEY = 'ecoalert_mock_notify_history';
const MOCK_ROI_KEY = 'ecoalert_mock_roi_config';
const MOCK_ALGO_KEY = 'ecoalert_mock_algorithm_config';
const GLOBAL_ROI_SOURCE_ID = '__global__';
const DEFAULT_VLM_PROMPT = `你是一个专业的人体目标检测系统。请仔细分析这张图片，检测其中是否包含人体（完整或局部均可，包括背影、侧身、被部分遮挡的人）。

你必须严格按照以下 JSON 格式输出，不要包含任何额外文字、解释或说明：

当检测到人时：
{"has_person": true, "detections": [{"label": "person", "confidence": 0.95, "bbox": [x1, y1, x2, y2]}]}

当未检测到人时：
{"has_person": false, "detections": []}

要求：
1. bbox 坐标采用千分制归一化值（范围 0-1000），[x1, y1] 为边界框左上角，[x2, y2] 为右下角
2. confidence 为 0-1 之间的浮点数，表示检测置信度
3. 检测到的每一个人都必须单独列出一条记录
4. 仅输出 JSON，不要包含 markdown 标记、代码块或其他任何文字`;

function normalizeSource(s) {
  if (!s) return s;
  return {
    ...s,
    type: s.type ?? s.source_type,
    groupId: s.groupId ?? s.group_id ?? 'grp-default',
    createdAt: s.createdAt ?? s.created_at,
  };
}

function normalizeGroup(g) {
  if (!g) return g;
  return {
    ...g,
    createdAt: g.createdAt ?? g.created_at,
    domainDetectionEnabled: g.domainDetectionEnabled ?? g.domain_detection_enabled ?? false,
  };
}

function normalizeStateRecord(r) {
  if (!r) return r;
  return {
    ...r,
    sourceId: r.sourceId ?? r.source_id,
  };
}

function normalizeSceneState(payload) {
  if (!payload) return payload;
  const alarmStatus = payload.alarmStatus ?? payload.alarm_status ?? 'normal';
  const alarmRecordActive = payload.alarmRecordActive ?? payload.alarm_record_active ?? false;
  return {
    ...payload,
    sourceId: payload.sourceId ?? payload.source_id,
    personConfidence: payload.personConfidence ?? payload.person_confidence ?? 0,
    lightConfidence: payload.lightConfidence ?? payload.light_confidence ?? 0,
    lightState: payload.lightState ?? payload.light_state ?? (payload.light ? 'on' : 'off'),
    simplePerson: payload.simplePerson ?? payload.simple_person ?? payload.person,
    simplePersonConfidence: payload.simplePersonConfidence ?? payload.simple_person_confidence ?? payload.person_confidence ?? 0,
    vlmPerson: payload.vlmPerson ?? payload.vlm_person ?? null,
    vlmPersonConfidence: payload.vlmPersonConfidence ?? payload.vlm_person_confidence ?? null,
    vlmStatus: payload.vlmStatus ?? payload.vlm_status ?? 'none',
    alarm: !!payload.alarm || ['alarm_active', 'acknowledged', 'recovering'].includes(alarmStatus),
    alarmStatus,
    alarmRecordActive: !!alarmRecordActive,
    alarmProgress: payload.alarmProgress ?? payload.alarm_progress ?? 0,
    vlmProgress: payload.vlmProgress ?? payload.vlm_progress ?? 0,
    alarmCountdownProgress: payload.alarmCountdownProgress ?? payload.alarm_countdown_progress ?? 0,
    source: payload.source ?? 'simple',
    modelLatencyMs: payload.modelLatencyMs ?? payload.model_latency_ms ?? null,
    frameSeq: payload.frameSeq ?? payload.frame_seq ?? 0,
    confidence: payload.confidence ?? 0,
    reason: payload.reason ?? null,
    lightBrightness: payload.lightBrightness ?? payload.light_brightness ?? null,
    colorScore: payload.colorScore ?? payload.color_score ?? null,
    motionScore: payload.motionScore ?? payload.motion_score ?? null,
    processMs: payload.processMs ?? payload.process_ms ?? null,
  };
}

function normalizeDetectionSample(payload) {
  const s = normalizeSceneState(payload);
  return {
    ...s,
    sourceId: payload.sourceId ?? payload.source_id,
    frameSeq: payload.frameSeq ?? payload.frame_seq ?? 0,
    alarmStatus: payload.alarmStatus ?? payload.alarm_status ?? 'normal',
    lightBrightness: payload.lightBrightness ?? payload.light_brightness ?? 0,
    colorScore: payload.colorScore ?? payload.color_score ?? 0,
    motionScore: payload.motionScore ?? payload.motion_score ?? 0,
    processMs: payload.processMs ?? payload.process_ms ?? 0,
    ts: payload.ts,
  };
}

function normalizeRuntimeStatus(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    sourceId: payload.sourceId ?? payload.source_id,
    onlineStatus: payload.onlineStatus ?? payload.online_status,
    algorithmStatus: payload.algorithmStatus ?? payload.algorithm_status,
    alarmStatus: payload.alarmStatus ?? payload.alarm_status,
    vlmEnabled: payload.vlmEnabled ?? payload.vlm_enabled ?? false,
    lastFrameAt: payload.lastFrameAt ?? payload.last_frame_at,
    lastAlgorithmAt: payload.lastAlgorithmAt ?? payload.last_algorithm_at,
    lastError: payload.lastError ?? payload.last_error,
    effectiveAlgorithmConfigScope: payload.effectiveAlgorithmConfigScope ?? payload.effective_algorithm_config_scope,
  };
}

function normalizeAlgorithmSchedule(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    sourceId: payload.sourceId ?? payload.source_id,
    latencyMs: payload.latencyMs ?? payload.latency_ms,
  };
}

function normalizeAlarmRecord(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    sourceId: payload.sourceId ?? payload.source_id,
    firstSeenAt: payload.firstSeenAt ?? payload.first_seen_at,
    triggeredAt: payload.triggeredAt ?? payload.triggered_at,
    acknowledgedAt: payload.acknowledgedAt ?? payload.acknowledged_at,
    resolvedAt: payload.resolvedAt ?? payload.resolved_at,
    acknowledgedBy: payload.acknowledgedBy ?? payload.acknowledged_by,
    lastStateId: payload.lastStateId ?? payload.last_state_id,
  };
}

function normalizeAlarmEvent(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    alarmId: payload.alarmId ?? payload.alarm_id,
    sourceId: payload.sourceId ?? payload.source_id,
  };
}

function normalizeNotificationRecord(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    targetId: payload.targetId ?? payload.target_id,
    targetName: payload.targetName ?? payload.target_name,
    sourceId: payload.sourceId ?? payload.source_id,
    alarmId: payload.alarmId ?? payload.alarm_id,
    statusCode: payload.statusCode ?? payload.status_code,
    requestAt: payload.requestAt ?? payload.request_at,
    latencyMs: payload.latencyMs ?? payload.latency_ms,
    retryCount: payload.retryCount ?? payload.retry_count,
    requestBody: payload.requestBody ?? payload.request_body,
  };
}

function normalizeNotificationEvent(payload) {
  if (!payload) return payload;
  return {
    ...payload,
    recordId: payload.recordId ?? payload.record_id,
    targetId: payload.targetId ?? payload.target_id,
  };
}

function toSourcePayload(payload) {
  return {
    name: payload.name,
    url: payload.url,
    type: payload.type ?? payload.sourceType ?? payload.source_type,
    location: payload.location ?? '',
    enabled: !!payload.enabled,
    group_id: payload.groupId ?? payload.group_id ?? 'grp-default',
    order: payload.order ?? 0,
  };
}

function toOrderItems(items) {
  return (items || []).map((it) => ({
    id: it.id,
    order: it.order,
    group_id: it.groupId ?? it.group_id,
  }));
}

function toGroupPayload(payload) {
  return {
    name: payload.name,
    order: payload.order ?? 0,
    collapsed: !!payload.collapsed,
    domain_detection_enabled: !!(payload.domainDetectionEnabled ?? payload.domain_detection_enabled),
  };
}

function mockLoad() {
  // 浏览器预览模式：优先读 localStorage（保留用户修改），否则返回默认数据
  try {
    const raw = localStorage.getItem(MOCK_KEY);
    const version = localStorage.getItem(MOCK_SOURCE_VERSION_KEY);
    if (raw && version === MOCK_SOURCE_VERSION) {
      const arr = JSON.parse(raw);
      if (Array.isArray(arr) && arr.length > 0) return arr;
    }
  } catch (e) { console.warn('[mock] mockLoad 解析失败:', e); }
  const defaults = mockDefaultSources();
  mockSave(defaults);
  return defaults;
}

function mockDefaultSources() {
  // 本地 HLS 推流地址（与 Tools/push_streamer 统一命名 cam-1 ~ cam-8）
  // 启动推流器后生效：python -m push_streamer.cli --auto-scan
  const HLS = (n) => `http://127.0.0.1:8080/cam-${n}/index.m3u8`;
  return [
    { id: 'cam-domain-0424',  name: '4·24 域控', url: HLS(1), type: 'hls', location: 'Video/4·24域控.mp4', enabled: true, groupId: 'grp-domain',  order: 0, createdAt: Date.now() - 80000000 },
    { id: 'cam-domain-0527',  name: '5·27 域控', url: HLS(5), type: 'hls', location: 'Video/5·27域控.mp4', enabled: true, groupId: 'grp-domain',  order: 1, createdAt: Date.now() - 70000000 },
    { id: 'cam-domain-0528',  name: '5·28 域控', url: HLS(6), type: 'hls', location: 'Video/5·28域控.mp4', enabled: true, groupId: 'grp-domain',  order: 2, createdAt: Date.now() - 70000000 },
    { id: 'cam-domain-0507',  name: '5·7 域控',  url: HLS(7), type: 'hls', location: 'Video/5·7域控.mp4',  enabled: true, groupId: 'grp-domain',  order: 3, createdAt: Date.now() - 60000000 },
    { id: 'cam-chassis-0424', name: '4·24 底盘', url: HLS(2), type: 'hls', location: 'Video/4·24底盘.mp4', enabled: true, groupId: 'grp-chassis', order: 0, createdAt: Date.now() - 80000000 },
    { id: 'cam-chassis-0515', name: '5·15 底盘', url: HLS(4), type: 'hls', location: 'Video/5·15底盘.mp4', enabled: true, groupId: 'grp-chassis', order: 1, createdAt: Date.now() - 80000000 },
    { id: 'cam-chassis-0507', name: '5·7 底盘',  url: HLS(8), type: 'hls', location: 'Video/5·7底盘.mp4',  enabled: true, groupId: 'grp-chassis', order: 2, createdAt: Date.now() - 50000000 },
    { id: 'cam-hardware-0514', name: '5·14 硬件', url: HLS(3), type: 'hls', location: 'Video/5·14硬件.mp4', enabled: true, groupId: 'grp-hardware', order: 0, createdAt: Date.now() - 80000000 },
  ];
}
function mockSave(arr) {
  localStorage.setItem(MOCK_KEY, JSON.stringify(arr));
  localStorage.setItem(MOCK_SOURCE_VERSION_KEY, MOCK_SOURCE_VERSION);
}
function mockLoadGroups() {
  try {
    const raw = localStorage.getItem(MOCK_GROUPS_KEY);
    const version = localStorage.getItem(MOCK_GROUP_VERSION_KEY);
    if (raw && version === MOCK_GROUP_VERSION) {
      const arr = JSON.parse(raw);
      if (Array.isArray(arr) && arr.length > 0) return arr;
    }
  } catch (e) { console.warn('[mock] mockLoadGroups 解析失败:', e); }
  const defaults = [
    { id: 'grp-domain',   name: '域控测试视频', order: 0, collapsed: false, domainDetectionEnabled: false, createdAt: Date.now() },
    { id: 'grp-chassis',  name: '底盘测试视频', order: 1, collapsed: false, domainDetectionEnabled: false, createdAt: Date.now() },
    { id: 'grp-hardware', name: '硬件测试视频', order: 2, collapsed: false, domainDetectionEnabled: false, createdAt: Date.now() },
  ];
  mockSaveGroups(defaults);
  return defaults;
}
function mockSaveGroups(arr) {
  localStorage.setItem(MOCK_GROUPS_KEY, JSON.stringify(arr));
  localStorage.setItem(MOCK_GROUP_VERSION_KEY, MOCK_GROUP_VERSION);
}
function mockLoadHistory(sourceId, limit) {
  try {
    if (localStorage.getItem(MOCK_HISTORY_VERSION_KEY) !== MOCK_HISTORY_VERSION) {
      localStorage.removeItem(MOCK_HISTORY_KEY);
      localStorage.setItem(MOCK_HISTORY_VERSION_KEY, MOCK_HISTORY_VERSION);
    }
    const raw = localStorage.getItem(MOCK_HISTORY_KEY);
    const all = raw ? JSON.parse(raw) : [];
    let out = sourceId ? all.filter((r) => r.sourceId === sourceId) : all;
    out = out.slice(-limit).reverse();
    const bySource = {};
    for (const r of out) {
      (bySource[r.sourceId] = bySource[r.sourceId] || []).push(r);
    }
    return { ok: true, records: out, bySource };
  } catch (_) {
    return { ok: true, records: [], bySource: {} };
  }
}
function mockGetPw() {
  return localStorage.getItem(MOCK_PW_KEY) || MOCK_PASSWORD;
}
function mockSetPw(p) {
  localStorage.setItem(MOCK_PW_KEY, p);
}

/* ---------- 通知目标 mock（持久化到 localStorage，预置一个企业内部通知） ---------- */
function mockLoadNtf() {
  try {
    const raw = localStorage.getItem(MOCK_NTF_KEY);
    if (raw) {
      const arr = JSON.parse(raw);
      // 旧 cache 是空数组 / 不含预置 id → 重置为默认
      if (Array.isArray(arr) && arr.length === 0) {
        // fall through to default
      } else if (Array.isArray(arr) && arr.some((x) => x && x.id === 'ntf-default-hirain')) {
        return arr;
      } else if (Array.isArray(arr)) {
        // 已有用户自定义的目标，保留，但确保预置目标也存在
        const hasDefault = arr.some((x) => x && x.id === 'ntf-default-hirain');
        if (!hasDefault) {
          arr.unshift(buildDefaultNtfTarget());
          mockSaveNtf(arr);
        }
        return arr;
      }
    }
  } catch (e) { console.warn('[mock] mockLoadNtf 解析失败:', e); }
  // 预置默认通知：适配自企业内推
  return [buildDefaultNtfTarget()];
}

function buildDefaultNtfTarget() {
  return {
    id: 'ntf-default-hirain',
    name: '企业内部通知（适配示例）',
    enabled: true,
    url: 'https://biz.hirain.com/synergy/notice/545B6B5FEF17',
    method: 'POST',
    headers: [
      { name: 'Content-Type', value: 'application/json' },
      { name: 'Cookie', value: 'JSESSIONID=1D6DEE38ECAC44223748CE0B062F8CC0' },
    ],
    eventTypes: ['alarm_triggered', 'alarm_resolved'],
    cooldownSec: 1800,
    timeoutSec: 10,
    retryCount: 2,
    bodyTemplate: JSON.stringify({
      touser: 'jinsheng.liu1',
      msgtype: 'text',
      agentcode: 'ai_challenge',
      text: { content: '[EcoAlert] {{event}}\n视频源: {{source_name}}\n区域: {{location}}\n有人: {{person}}\n亮灯: {{light}}\n时间: {{ts_formatted}}' },
      subject: '{{subject}}',
      from: '{{from}}',
    }, null, 2),
    textTemplates: {
      alarm_triggered:
        '[告警] 通道「{{source_name}}」在 {{location}} 触发告警：无人 + 亮灯<br>请及时处理。',
      alarm_resolved:
        '[已恢复] 通道「{{source_name}}」在 {{location}} 状态已恢复正常<br>告警持续 {{duration}}',
      test: '【EcoAlert 测试】这是一条来自 EcoAlert 的测试通知，时间 {{ts}}',
    },
    subjectTemplates: {
      alarm_triggered: '告警：{{source_name}}',
      alarm_resolved: '已恢复：{{source_name}}',
      test: 'EcoAlert 测试通知',
    },
    fromName: 'EcoAlert 监控系统',
    createdAt: Date.now(),
  };
}
function mockSaveNtf(arr) {
  localStorage.setItem(MOCK_NTF_KEY, JSON.stringify(arr));
}
function mockLoadNtfHistory() {
  try {
    const raw = localStorage.getItem(MOCK_NTF_HISTORY_KEY);
    if (raw) return JSON.parse(raw);
  } catch (e) { console.warn('[mock] mockLoadNtfHistory 解析失败:', e); }
  return [];
}
function mockSaveNtfHistory(arr) {
  localStorage.setItem(MOCK_NTF_HISTORY_KEY, JSON.stringify(arr));
}

/* ---------- 诊断工具 ---------- */
export async function openDevtools() {
  if (isTauri) return invoke('open_devtools');
}
export async function probeUrl(url) {
  if (isTauri) return invoke('probe_url', { url });
  // 浏览器 mock：fetch 一下
  try {
    const r = await fetch(url, { method: 'HEAD' });
    return { ok: r.ok, status: r.status, content_length: Number(r.headers.get('content-length') || 0) };
  } catch (e) {
    return { ok: false, status: 0, content_length: 0, error: String(e) };
  }
}
export async function checkFfmpegStatus() {
  if (isTauri) return invoke('check_ffmpeg_status');
  return {
    ok: false,
    ffmpeg: { ok: false, path: 'ffmpeg', version: null, error: '浏览器预览模式不可检测 ffmpeg' },
    ffprobe: { ok: false, path: 'ffprobe', version: null, error: '浏览器预览模式不可检测 ffprobe' },
  };
}

/* ---------- 命令封装 ---------- */
export async function login(password) {
  if (isTauri) return invoke('login', { password });
  // 浏览器 mock
  return new Promise((resolve, reject) => {
    setTimeout(() => {
      if (password === mockGetPw()) {
        resolve({ ok: true, token: 'mock-token' });
      } else {
        reject(new Error('密码错误'));
      }
    }, 200);
  });
}

export async function logout() {
  if (isTauri) return invoke('logout');
  return { ok: true };
}

export async function checkAuth() {
  if (isTauri) return invoke('check_auth');
  return { ok: true };
}

export async function listSources() {
  if (isTauri) return (await invoke('list_sources')).map(normalizeSource);
  return mockLoad();
}

export async function listGroups() {
  if (isTauri) return (await invoke('list_groups')).map(normalizeGroup);
  return mockLoadGroups().map(normalizeGroup);
}

export async function createGroup(payload) {
  if (isTauri) return normalizeGroup(await invoke('create_group', { payload: toGroupPayload(payload) }));
  const all = mockLoadGroups();
  const item = {
    id: 'grp-' + Math.random().toString(36).slice(2, 10),
    ...payload,
    createdAt: Date.now(),
  };
  all.push(item);
  mockSaveGroups(all);
  return normalizeGroup(item);
}

export async function updateGroup(id, payload) {
  if (isTauri) return normalizeGroup(await invoke('update_group', { id, payload: toGroupPayload(payload) }));
  const all = mockLoadGroups();
  const idx = all.findIndex((g) => g.id === id);
  if (idx < 0) throw new Error('分组不存在');
  all[idx] = { ...all[idx], ...payload };
  mockSaveGroups(all);
  return normalizeGroup(all[idx]);
}

export async function deleteGroup(id) {
  if (isTauri) return invoke('delete_group', { id });
  let all = mockLoadGroups().filter((g) => g.id !== id);
  if (all.length === 0) all = [{ id: 'grp-default', name: '默认分组', order: 0, collapsed: false, domainDetectionEnabled: false, createdAt: Date.now() }];
  mockSaveGroups(all);
  // 把该组下的源移到默认组
  const sources = mockLoad().map((s) => s.groupId === id ? { ...s, groupId: 'grp-default' } : s);
  mockSave(sources);
  return { ok: true };
}

export async function reorder(items) {
  if (isTauri) return invoke('reorder', { items: toOrderItems(items) });
  const all = mockLoad();
  for (const it of items) {
    const s = all.find((x) => x.id === it.id);
    if (s) {
      s.order = it.order;
      if (it.groupId) s.groupId = it.groupId;
    }
  }
  mockSave(all);
  return { ok: true };
}

export async function createSource(payload) {
  if (isTauri) return normalizeSource(await invoke('create_source', { payload: toSourcePayload(payload) }));
  const all = mockLoad();
  const item = {
    id: 'src-' + Math.random().toString(36).slice(2, 10),
    order: 0,
    ...payload,
    createdAt: Date.now(),
  };
  all.push(item);
  mockSave(all);
  return item;
}

export async function updateSource(id, payload) {
  if (isTauri) return normalizeSource(await invoke('update_source', { id, payload: toSourcePayload(payload) }));
  const all = mockLoad();
  const idx = all.findIndex((s) => s.id === id);
  if (idx < 0) throw new Error('视频源不存在');
  all[idx] = { ...all[idx], ...payload };
  mockSave(all);
  return all[idx];
}

export async function deleteSource(id) {
  if (isTauri) return invoke('delete_source', { id });
  const all = mockLoad().filter((s) => s.id !== id);
  mockSave(all);
  return { ok: true };
}

export async function importTestSourcesFromFolder(folderPath) {
  if (isTauri) {
    const result = await invoke('import_test_sources_from_folder', { folderPath });
    return {
      ...result,
      sources: (result.sources || []).map(normalizeSource),
    };
  }
  return { sources: mockLoad().map(normalizeSource), imported: 0, skipped: 0 };
}

/// 调试菜单"测试视频源"开关：enabled=true 创建 8 个预设测试源，false 仅删除这 8 个。
export const TEST_SOURCE_IDS = [
  'cam-domain-0424', 'cam-domain-0527', 'cam-domain-0528', 'cam-domain-0507',
  'cam-chassis-0424', 'cam-chassis-0515', 'cam-chassis-0507', 'cam-hardware-0514',
];

export async function setTestSourcesEnabled(enabled) {
  if (isTauri) {
    const list = await invoke('set_test_sources_enabled', { enabled });
    return list.map(normalizeSource);
  }
  if (enabled) {
    const existing = mockLoad();
    const existingIds = new Set(existing.map((s) => s.id));
    const seeded = mockDefaultSources().filter((s) => !existingIds.has(s.id));
    mockSave([...existing, ...seeded]);
  } else {
    const removeIds = new Set(TEST_SOURCE_IDS);
    mockSave(mockLoad().filter((s) => !removeIds.has(s.id)));
  }
  return mockLoad().map(normalizeSource);
}

export async function reportSceneState(sourceId, person, light) {
  if (isTauri) return invoke('report_scene_state', { sourceId, person, light });
  // mock 模式下不做任何事
  return { ok: true };
}

export async function getStateHistory(sourceId, limit) {
  if (isTauri) {
    const res = await invoke('get_state_history', { sourceId: sourceId ?? null, limit: limit ?? 100 });
    const records = (res.records || []).map(normalizeStateRecord);
    const bySource = {};
    for (const [id, list] of Object.entries(res.bySource || res.by_source || {})) {
      bySource[id] = (list || []).map(normalizeStateRecord);
    }
    return { ...res, records, bySource };
  }
  // mock 模式从 localStorage 读
  return mockLoadHistory(sourceId, limit);
}

export async function listDetectionHistory(sourceId = null, limit = 500) {
  if (isTauri) {
    const res = await invoke('list_detection_history', { sourceId, limit });
    return (res.records || []).map(normalizeDetectionSample);
  }
  return [];
}

export async function getChannelRuntimeStatus(sourceId = null) {
  if (isTauri) {
    const list = await invoke('get_channel_runtime_status', { sourceId });
    return (list || []).map(normalizeRuntimeStatus);
  }
  const now = Date.now();
  return mockLoad()
    .filter((s) => !sourceId || s.id === sourceId)
    .map((s) => ({
      sourceId: s.id,
      onlineStatus: s.enabled ? 'online' : 'offline',
      algorithmStatus: 'disabled',
      alarmStatus: 'normal',
      lastFrameAt: s.enabled ? now : null,
      lastAlgorithmAt: null,
      lastError: '浏览器预览模式不运行检测，请使用 Tauri 应用查看真实算法结果',
      effectiveAlgorithmConfigScope: 'global',
      ts: now,
    }));
}

export async function listAlarms({ status = null, sourceId = null, limit = 100 } = {}) {
  if (isTauri) {
    const list = await invoke('list_alarms', { status, sourceId, limit });
    return (list || []).map(normalizeAlarmRecord);
  }
  return [];
}

export async function ackAlarm(alarmId, note = null) {
  if (isTauri) return normalizeAlarmRecord(await invoke('ack_alarm', { alarmId, note }));
  return { id: alarmId, status: 'acknowledged', acknowledgedAt: Date.now(), note };
}

export async function resolveAlarm(alarmId, note = null) {
  if (isTauri) return normalizeAlarmRecord(await invoke('resolve_alarm', { alarmId, note }));
  return { id: alarmId, status: 'resolved', resolvedAt: Date.now(), note };
}

export async function getAlgorithmConfig(sourceId = null) {
  if (isTauri) return invoke('get_algorithm_config', { sourceId });
  const defaults = {
    enabled: true,
    developerMode: false,
    scope: 'global',
    scopeId: null,
    activeWindows: [],
    exceptionWindows: [],
    simpleIntervalSec: 1,
    vlmIntervalSec: 300,
    vlmEnabled: false,
    vlmSkipWhenPerson: true,
    vlmApiBase: '',
    vlmApiKey: '',
    vlmModel: '',
    vlmPrompt: DEFAULT_VLM_PROMPT,
    vlmTemperature: 0.1,
    vlmMaxTokens: 2048,
    vlmCostEnabled: false,
    vlmPriceInput: 0,
    vlmPriceInputCache: 0,
    vlmPriceOutput: 0,
    vlmPriceOutputCache: 0,
    personThreshold: 0.003,
    lightThreshold: 0.7,
    alarmHoldSec: 300,
    alarmRecoverSec: 60,
    recoverPolicy: 'either',
    vlmHourlyLimit: 12,
    roiVersion: null,
  };
  try {
    const raw = localStorage.getItem(MOCK_ALGO_KEY);
    const file = raw ? JSON.parse(raw) : {};
    if (sourceId && file.sources?.[sourceId]) return file.sources[sourceId];
    return file.global || defaults;
  } catch (_) {
    return defaults;
  }
}

export async function updateAlgorithmConfig(sourceId, payload) {
  if (isTauri) return invoke('update_algorithm_config', { sourceId: sourceId ?? null, payload });
  const saved = { ...payload, scope: sourceId ? 'source' : 'global', scopeId: sourceId ?? null };
  const raw = localStorage.getItem(MOCK_ALGO_KEY);
  const file = raw ? JSON.parse(raw) : { sources: {} };
  file.sources = file.sources || {};
  if (sourceId) file.sources[sourceId] = saved;
  else file.global = saved;
  localStorage.setItem(MOCK_ALGO_KEY, JSON.stringify(file));
  return saved;
}

export async function listAlgorithmConfigSources() {
  if (isTauri) return invoke('list_algorithm_config_sources');
  try {
    const raw = localStorage.getItem(MOCK_ALGO_KEY);
    const file = raw ? JSON.parse(raw) : {};
    return Object.keys(file.sources || {}).sort();
  } catch (_) {
    return [];
  }
}

export async function getEffectiveAlgorithmConfig(sourceId) {
  if (isTauri) return invoke('get_effective_algorithm_config', { sourceId });
  const config = await getAlgorithmConfig(sourceId);
  return { config, scope: config.scope || 'global' };
}

export async function deleteAlgorithmConfig(sourceId) {
  if (isTauri) return invoke('delete_algorithm_config', { sourceId });
  const raw = localStorage.getItem(MOCK_ALGO_KEY);
  const file = raw ? JSON.parse(raw) : { sources: {} };
  if (file.sources) delete file.sources[sourceId];
  localStorage.setItem(MOCK_ALGO_KEY, JSON.stringify(file));
  return { ok: true };
}

function calcVlmCost(usage, cfg) {
  if (!cfg?.vlmCostEnabled && !cfg?.vlm_cost_enabled) return null;
  if (!usage) return 0;
  const normalInput = Math.max(0, (usage.promptTokens || usage.prompt_tokens || 0) - (usage.promptCachedTokens || usage.prompt_cached_tokens || 0));
  const cachedInput = usage.promptCachedTokens || usage.prompt_cached_tokens || 0;
  const normalOutput = Math.max(0, (usage.completionTokens || usage.completion_tokens || 0) - (usage.completionCachedTokens || usage.completion_cached_tokens || 0));
  const cachedOutput = usage.completionCachedTokens || usage.completion_cached_tokens || 0;
  return (
    normalInput * (cfg.vlmPriceInput || 0) +
    cachedInput * (cfg.vlmPriceInputCache || 0) +
    normalOutput * (cfg.vlmPriceOutput || 0) +
    cachedOutput * (cfg.vlmPriceOutputCache || 0)
  ) / 1_000_000;
}

export async function testVlmConfig(payload) {
  if (isTauri) return invoke('test_vlm_config', { payload });
  const usage = {
    promptTokens: 128,
    completionTokens: 8,
    totalTokens: 136,
    promptCachedTokens: 0,
    completionCachedTokens: 0,
  };
  return {
    ok: true,
    reply: `mock ok: ${payload.vlmModel || '未填写模型'}`,
    usage,
    costEnabled: !!(payload?.vlmCostEnabled ?? payload?.vlm_cost_enabled),
    cost: calcVlmCost(usage, payload || {}),
  };
}

/// 用指定视频源的画面做真实 VLM 图片识别测试
export async function testVlmVision(payload) {
  if (isTauri) return invoke('test_vlm_vision', { payload });
  return {
    ok: true,
    reply: `mock vision ok: ${payload.vlmModel || '未填写模型'} (source=${payload.sourceId})`,
    usage: null,
  };
}

export async function testYoloConnection(apiBase) {
  if (isTauri) return invoke('test_yolo_connection', { apiBase });
  return {
    ok: true,
    count: 0,
    processMs: 12.3,
    url: `${(apiBase || 'ws://localhost:8090').replace(/^http/, 'ws')}/ws`,
  };
}

export async function getRoiConfig(sourceId) {
  if (isTauri) return invoke('get_roi_config', { sourceId });
  try {
    const raw = localStorage.getItem(MOCK_ROI_KEY);
    const all = raw ? JSON.parse(raw) : {};
    const bySource = all.bySource || all.by_source || all;
    const global = all.global || bySource[GLOBAL_ROI_SOURCE_ID];
    if (!sourceId || sourceId === GLOBAL_ROI_SOURCE_ID) {
      if (global) return global;
    }
    if (bySource[sourceId]) return bySource[sourceId];
    if (global) return { ...global, sourceId };
  } catch (e) { console.warn('[mock] ROI 配置读取失败:', e); }
  return {
    sourceId: sourceId || GLOBAL_ROI_SOURCE_ID,
    version: 'mock-roi',
    lightRois: [{ id: 'light-main', label: '全屏', x: 0, y: 0, w: 1, h: 1 }],
    excludeRois: [],
    personRois: [],
    lightOnThreshold: 0.055,
    lightOffThreshold: 0.025,
    updatedAt: Date.now(),
  };
}

export async function listRoiConfigSources() {
  if (isTauri) return invoke('list_roi_config_sources');
  try {
    const raw = localStorage.getItem(MOCK_ROI_KEY);
    const parsed = raw ? JSON.parse(raw) : {};
    const bySource = parsed.bySource || parsed.by_source || parsed;
    return Object.keys(bySource)
      .filter((id) => id && !['global', 'bySource', 'by_source', GLOBAL_ROI_SOURCE_ID].includes(id))
      .sort();
  } catch (e) { console.warn('[mock] ROI 配置列表读取失败:', e); }
  return [];
}

export async function updateRoiConfig(sourceId, payload) {
  if (isTauri) return invoke('update_roi_config', { sourceId, payload });
  const isGlobal = !sourceId || sourceId === GLOBAL_ROI_SOURCE_ID;
  const saved = { ...payload, sourceId: sourceId || GLOBAL_ROI_SOURCE_ID, updatedAt: Date.now() };
  try {
    const raw = localStorage.getItem(MOCK_ROI_KEY);
    const parsed = raw ? JSON.parse(raw) : {};
    const bySource = parsed.bySource || parsed.by_source || {};
    const all = { global: parsed.global || bySource[GLOBAL_ROI_SOURCE_ID], bySource };
    if (isGlobal) all.global = saved;
    else all.bySource[sourceId] = saved;
    localStorage.setItem(MOCK_ROI_KEY, JSON.stringify(all));
  } catch (e) { console.warn('[mock] ROI 配置写入失败:', e); }
  return saved;
}

export async function deleteRoiConfig(sourceId) {
  if (isTauri) return invoke('delete_roi_config', { sourceId });
  try {
    const raw = localStorage.getItem(MOCK_ROI_KEY);
    const parsed = raw ? JSON.parse(raw) : {};
    const bySource = parsed.bySource || parsed.by_source || parsed;
    delete bySource[sourceId];
    localStorage.setItem(MOCK_ROI_KEY, JSON.stringify({
      global: parsed.global || bySource[GLOBAL_ROI_SOURCE_ID],
      bySource,
    }));
  } catch (e) { console.warn('[mock] ROI 配置删除失败:', e); }
  return { ok: true };
}

export async function testRoiConfig(sourceId, payload = null) {
  if (isTauri) return invoke('test_roi_config', { sourceId, payload });
  const roi = (payload?.lightRois || [])[0] || { x: 0, y: 0, w: 1, h: 1 };
  const cx = roi.x + roi.w / 2;
  const cy = roi.y + roi.h / 2;
  const hitsBrightPatch = cx >= 0.3 && cx <= 0.7 && cy >= 0.27 && cy <= 0.73;
  const brightness = hitsBrightPatch ? 230 : 28;
  const colorScore = hitsBrightPatch ? 0.08 : 0.01;
  const light = colorScore >= (payload?.lightOnThreshold ?? 0.055);
  return {
    ok: true,
    light,
    lightState: light ? 'on' : 'off',
    person: false,
    brightness,
    colorScore,
    motionScore: 0,
    confidence: light ? 0.9 : 0.7,
    processMs: 0.1,
    version: payload?.version || 'mock-roi',
  };
}

export async function listNotificationTargets() {
  if (isTauri) return invoke('list_notification_targets');
  return mockLoadNtf();
}

export async function createNotificationTarget(payload) {
  if (isTauri) return invoke('create_notification_target', { payload });
  const all = mockLoadNtf();
  const item = { id: 'ntf-' + Math.random().toString(36).slice(2, 10), createdAt: Date.now(), ...payload };
  all.push(item);
  mockSaveNtf(all);
  return item;
}

export async function updateNotificationTarget(id, payload) {
  if (isTauri) return invoke('update_notification_target', { id, payload });
  const all = mockLoadNtf();
  const idx = all.findIndex((x) => x.id === id);
  if (idx < 0) throw new Error('通知目标不存在');
  all[idx] = { ...all[idx], ...payload };
  mockSaveNtf(all);
  return all[idx];
}

export async function deleteNotificationTarget(id) {
  if (isTauri) return invoke('delete_notification_target', { id });
  const all = mockLoadNtf().filter((x) => x.id !== id);
  mockSaveNtf(all);
  return { ok: true };
}

export async function listNotificationHistory({ sourceId = null, event = null, ok = null, limit = 100 } = {}) {
  if (isTauri) {
    const list = await invoke('list_notification_history', { sourceId, event, ok, limit });
    return (list || []).map(normalizeNotificationRecord);
  }
  let arr = mockLoadNtfHistory();
  if (sourceId) arr = arr.filter((r) => r.sourceId === sourceId);
  if (event) arr = arr.filter((r) => r.event === event);
  if (ok !== null && ok !== undefined) arr = arr.filter((r) => !!r.ok === !!ok);
  return arr.slice(-limit).reverse();
}

export async function testNotificationTarget({ id = null, payload = null } = {}) {
  if (isTauri) {
    return normalizeNotificationRecord(await invoke('test_notification_target', { id, payload }));
  }
  // 浏览器 mock：记录一次测试发送，返回成功
  const rec = {
    id: 'nhr-' + Math.random().toString(36).slice(2, 10),
    targetId: id ?? 'mock-target',
    targetName: payload?.name ?? 'Mock',
    event: 'test',
    sourceId: null,
    sourceName: null,
    location: null,
    ok: true,
    statusCode: 200,
    requestAt: Date.now(),
    durationMs: 42,
    retryCount: 0,
    previewBody: payload?.bodyTemplate || '',
  };
  const arr = mockLoadNtfHistory();
  arr.push(rec);
  if (arr.length > 500) arr.splice(0, arr.length - 500);
  mockSaveNtfHistory(arr);
  return rec;
}

export async function resendNotification(recordId) {
  if (isTauri) return normalizeNotificationRecord(await invoke('resend_notification', { recordId }));
  return { id: recordId, ok: true, statusCode: 200, requestAt: Date.now() };
}

export async function getSecurityConfig() {
  if (isTauri) return invoke('get_security_config');
  return {
    schemaVersion: 1,
    externalVlmEnabled: false,
    saveVlmSnapshots: false,
    snapshotRetentionDays: 7,
    includeImageInNotification: false,
    blurPersonBeforeExternalSend: true,
  };
}

export async function updateSecurityConfig(payload) {
  if (isTauri) return invoke('update_security_config', { payload });
  return payload;
}

export async function resetAllAppData() {
  if (isTauri) return invoke('reset_all_app_data');
  mockSave([]);
  mockSaveGroups([{ id: 'grp-default', name: '默认分组', order: 0, collapsed: false, domainDetectionEnabled: false, createdAt: Date.now() }]);
  localStorage.removeItem(MOCK_HISTORY_KEY);
  localStorage.removeItem(MOCK_ALGO_KEY);
  localStorage.removeItem(MOCK_ROI_KEY);
  localStorage.removeItem(MOCK_NTF_KEY);
  localStorage.removeItem(MOCK_NTF_HISTORY_KEY);
  return { ok: true, sources: [] };
}

export async function changePassword(oldPw, newPw) {
  if (isTauri) return invoke('change_password', { oldPassword: oldPw, newPassword: newPw });
  if (oldPw !== mockGetPw()) throw new Error('当前密码错误');
  if (newPw.length < 6) throw new Error('新密码至少 6 位');
  mockSetPw(newPw);
  return { ok: true };
}

export async function getDataDir() {
  if (isTauri) return invoke('get_data_dir');
  return '(浏览器预览模式 - 数据保存在 localStorage)';
}

/* ---------- OAuth / 凭证验证 ---------- */
export async function startOAuthBinding(channelType, appId, appSecret) {
  if (!isTauri) {
    return { sessionId: 'mock-session', port: 12345, authUrl: 'https://mock', qrData: 'https://mock' };
  }
  return invoke('start_oauth_binding', { channelType, appId, appSecret });
}

export async function checkOAuthStatus(sessionId, appId, appSecret) {
  if (!isTauri) {
    return { status: 'pending' };
  }
  return invoke('check_oauth_status', { sessionId, appId, appSecret });
}

export async function verifyChannelCredentials(channelType, appId, appSecret) {
  if (!isTauri) {
    return { ok: true, message: '凭证验证通过（模拟）' };
  }
  return invoke('verify_channel_credentials', { channelType, appId, appSecret });
}

/* ---------- 事件订阅 ---------- */
// 后端会推送事件：event(日志) / status(码率) / sources(源变更) / scene_state(算法)
export async function onEvent(handler) {
  if (isTauri) return listen('ecoalert://event', (e) => handler(e.payload));
  // 浏览器 mock: 推一些假状态
  const timer = setInterval(() => {
    handler({
      type: 'event',
      level: 'info',
      text: '演示事件：状态模拟推送（仅浏览器预览）',
    });
  }, 8000);
  return () => clearInterval(timer);
}
export async function onStatus(handler) {
  if (isTauri) return listen('ecoalert://status', (e) => handler(e.payload));
  const timer = setInterval(() => {
    const all = mockLoad();
    handler(
      all.map((s, i) => ({
        id: s.id,
        name: s.name,
        online: s.enabled,
        bitrate: s.enabled ? 800 + Math.floor(Math.random() * 2000) : 0,
        fps: s.enabled ? 20 + Math.floor(Math.random() * 10) : 0,
        viewers: s.enabled ? Math.floor(Math.random() * 6) : 0,
        location: s.location,
        ts: Date.now(),
      }))
    );
  }, 3000);
  return () => clearInterval(timer);
}
export async function onRuntimeStatus(handler) {
  if (isTauri) {
    return listen('ecoalert://runtime_status', (e) => {
      handler((e.payload || []).map(normalizeRuntimeStatus));
    });
  }
  const timer = setInterval(async () => {
    handler(await getChannelRuntimeStatus());
  }, 3000);
  return () => clearInterval(timer);
}
export async function onAlgorithmSchedule(handler) {
  if (isTauri) return listen('ecoalert://algorithm_schedule', (e) => handler(normalizeAlgorithmSchedule(e.payload)));
  return () => {};
}
export async function onAlarm(handler) {
  if (isTauri) return listen('ecoalert://alarm', (e) => handler(normalizeAlarmEvent(e.payload)));
  return () => {};
}
export async function onNotification(handler) {
  if (isTauri) return listen('ecoalert://notification', (e) => handler(normalizeNotificationEvent(e.payload)));
  return () => {};
}
export async function onSources(handler) {
  if (isTauri) return listen('ecoalert://sources', (e) => handler(e.payload));
  // 浏览器 mock 不再单独推 sources（手动 reload）
  return () => {};
}
export async function onSceneState(handler) {
  if (isTauri) return listen('ecoalert://scene_state', (e) => handler(normalizeSceneState(e.payload)));
  // 浏览器预览模式不伪造算法事件；本地 MP4 可由前端 Canvas 预览链路自行产生实时状态。
  return () => {};
}

export const isTauriEnv = isTauri;
