const $ = (selector) => document.querySelector(selector);
const state = { csrf: '', projects: [], project: null, detail: null, settings: {}, role: 'research', editing: false, timer: null };
const roles = { research: '调研策划', architect: '架构策划', writer: '文档执行策划', interaction: '交互策划', producer: '初级项目经理' };
const agentAnimations = {
  research: [`  o\n /|\\ \u{1F50D}\n / \\`, `  o.\n /|\\_\u{1F50D}\n /|`, ` _o\n |⟩\\ \u{1F50D}\n / \\`],
  architect: [` _o_\n |‾|_\u{1F4D0}\n / \\`, `  o\n /|\\ \u{1F4D0}\n / \\`, `  o\n /|⟨ \u{1F4D0}\n /|`],
  writer: [`  o\n /|\\\u{1F9F1}\u{1F9F1}\n / \\`, `  o_\n |\\ \u{1F9F1}\u{1F9F1}\n /|`, `  o\n |\\_\u{1F9F1}\u{1F9F1}\n / \\`],
  interaction: [`  o\n /|\\_\u2328\uFE0F\n / \\`, `  o/\n /| \u2328\uFE0F\n / \\`, `  o\n /|\\ \u261D\uFE0F\n /|`],
  producer: [`  o\n /|\\_\u{1F4CB}\n / \\`, `  o\n /| \u2713\u{1F4CB}\n /|`, ` _o\n |⟨ \u{1F4CB}\n / \\`],
};
const stages = [
  ['research', '调研分析', '寻找依据与撰写意见'], ['framework', '文档框架', '将意见落地为结构'],
  ['document', '文档撰写', '规则细化与参考图'], ['interaction', '交互补全', '状态、流程与线框图'],
  ['resources', '资源拆解', '功能、美术与音效'], ['review', '架构审核', '质量门禁与定稿'],
];
const statusText = { research_review: '请审阅调研意见', framework_review: '请确认文档框架', revision_review: '请确认修改影响范围', framework_approved: '框架已确认', interaction: '文档已生成，等待交互细化', resources: '交互已补全，等待资源拆解', review: '等待架构审核', approved: '审核通过，可导出', needs_human: '审核未通过，需要人工处理', interrupted: '执行中断，可检查并恢复' };
const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const toast = (message) => { $('#toast').textContent = message; $('#toast').classList.add('show'); clearTimeout(state.toast); state.toast = setTimeout(() => $('#toast').classList.remove('show'), 5500); };
async function api(path, options = {}) {
  const response = await fetch(path, { ...options, headers: { 'Content-Type': 'application/json', 'X-GDD-Token': state.csrf, ...(options.headers || {}) } });
  let body = {};
  try { body = await response.json(); } catch (_) {}
  if (!response.ok) throw new Error(body.detail || `请求失败 (${response.status})`);
  return body;
}
function openModal(content, wide = false) { $('#modal-content').innerHTML = content; $('#modal').classList.toggle('wide', wide); $('#modal-backdrop').hidden = false; }
function closeModal() { $('#modal-backdrop').hidden = true; $('#modal-content').innerHTML = ''; }
function showHome() { state.project = null; state.detail = null; clearTimeout(state.timer); stopRunAnimation(); $('#workspace').hidden = true; $('#home').hidden = false; renderProjects(); }
function statusBadge(status, simulated = false) { const waiting = status.includes('review') || status === 'needs_human'; const error = status === 'interrupted' || status === 'needs_human'; return `<span class="badge ${error ? 'error' : waiting ? 'waiting' : ''}">${esc(simulated && status === 'approved' ? '模拟审核 · 仅供验收' : statusText[status] || (status.startsWith('running:') ? '正在执行' : status))}</span>`; }
function startRunAnimation(roleKey) {
  stopRunAnimation();
  state.frameIdx = 0; state.tick = 0;
  const frames = agentAnimations[roleKey] || agentAnimations.research;
  $('#active-agent-icon').hidden = true;
  const pre = $('#agent-animation'); pre.hidden = false; pre.textContent = frames[0];
  state.animTimer = setInterval(() => {
    state.frameIdx += 1; state.tick += 1;
    pre.textContent = frames[state.frameIdx % frames.length];
    if (state.tick % 2 === 0) updateRunElapsed();
  }, 400);
  updateRunElapsed();
}
function stopRunAnimation() {
  if (state.animTimer) { clearInterval(state.animTimer); state.animTimer = null; }
  const pre = $('#agent-animation'); if (pre) { pre.hidden = true; pre.textContent = ''; }
  const icon = $('#active-agent-icon'); if (icon) icon.hidden = false;
  const elapsed = $('#run-elapsed'); if (elapsed) elapsed.textContent = '';
  state.runningSince = null;
}
function updateRunElapsed() {
  const el = $('#run-elapsed'); if (!el || !state.runningSince) return;
  const total = Math.max(0, Math.floor((Date.now() - state.runningSince) / 1000));
  const h = Math.floor(total / 3600), m = Math.floor(total % 3600 / 60), s = total % 60;
  const mm = String(m).padStart(2, '0'), ss = String(s).padStart(2, '0');
  el.textContent = `已运行 ${h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`}`;
}
function renderProjects() {
  $('#project-count').textContent = `${state.projects.length} 个项目`;
  if (!state.projects.length) return;
  $('#projects').innerHTML = state.projects.map((p) => `<button class="project-card" data-project="${p.id}">${statusBadge(p.status, p.artifacts.review?.content?.summary?.includes('本地模拟审核'))}<h3>${esc(p.name)}</h3><p>${esc(p.purpose)}</p><footer><span>参考：${esc(p.reference_text.slice(0, 30))}</span><span>第 ${p.revision} 版 →</span></footer></button>`).join('');
  document.querySelectorAll('[data-project]').forEach((el) => el.addEventListener('click', () => loadProject(el.dataset.project)));
}
function newProject() {
  openModal(`<h2 id="modal-title">新建策划文档</h2><p class="modal-intro">先说清楚想解决的设计问题与参考对象。调研 Agent 将据此检索资料，形成有来源的分析与撰写意见。</p><form id="create-form"><label class="field"><span>文档名称 *</span><input name="name" maxlength="120" required placeholder="例如：海岛营地生存玩法设计"></label><label class="field"><span>设计目的 *</span><textarea name="purpose" required placeholder="目标体验、需要解决的问题、期望的设计边界……"></textarea></label><label class="field"><span>参考对象 *</span><textarea name="references" required placeholder="游戏名称、具体版本或系统；可附参考链接"></textarea></label><div class="field-row"><label class="field"><span>目标平台</span><input name="platform" placeholder="PC / 移动端 / 主机"></label><label class="field"><span>目标受众</span><input name="audience" placeholder="核心玩家 / 休闲用户……"></label></div><div class="notice">创建后将启动调研，可能产生模型和搜索 API 调用费用。未配置密钥时会停在待配置状态，不会扣费。</div><div class="form-actions"><button type="button" class="secondary" data-close>取消</button><button class="primary" type="submit">创建并开始调研 →</button></div></form>`);
  $('#create-form').addEventListener('submit', async (event) => {
    event.preventDefault(); const data = Object.fromEntries(new FormData(event.target));
    const button = event.target.querySelector('[type=submit]'); button.disabled = true; button.textContent = '正在创建…';
    try { const result = await api('/api/projects', { method: 'POST', body: JSON.stringify(data) }); closeModal(); await refreshProjects(); await loadProject(result.project.id); } catch (error) { toast(error.message); button.disabled = false; button.textContent = '创建并开始调研 →'; }
  });
}
async function loadProject(id) {
  try { state.detail = await api(`/api/projects/${id}`); state.project = state.detail.project; $('#home').hidden = true; $('#workspace').hidden = false; renderWorkspace(); schedulePoll(); } catch (error) { toast(error.message); }
}
function schedulePoll() { clearTimeout(state.timer); if (state.project?.status.startsWith('running:')) state.timer = setTimeout(() => loadProject(state.project.id), 2500); }
function currentStage() {
  const p = state.project;
  if (p.status === 'running:revise' || p.status === 'needs_human') return 'review';
  if (p.status.startsWith('running:')) return p.status.split(':')[1] === 'revision_plan' ? 'research' : p.status.split(':')[1];
  if (p.status === 'research_review') return 'research'; if (p.status === 'revision_review') return 'research';
  if (p.status === 'framework_review' || p.status === 'framework_approved') return 'framework';
  return p.status === 'approved' ? 'review' : p.status;
}
function stageCompleted(key) {
  if (key === 'document') return !!state.project.artifacts.interaction;
  if (key === 'review') return state.project.status === 'approved';
  return !!state.project.artifacts[key];
}
function renderWorkspace() {
  const p = state.project;
  $('#project-title').textContent = p.name; $('#project-meta').textContent = `第 ${p.revision} 版 · ${p.reference_text.slice(0, 80)} · 审核修订 ${p.review_round}/3`;
  const stage = currentStage(); const role = stage === 'review' ? 'architect' : stage === 'research' || stage === 'revision_plan' ? 'research' : stage === 'framework' ? 'architect' : stage === 'document' ? 'writer' : stage === 'interaction' ? 'interaction' : stage === 'resources' ? 'producer' : 'architect';
  $('#active-agent').textContent = roles[role] || '架构策划';
  $('#stage-list').innerHTML = stages.map(([key, title, description], i) => `<li data-number="${i+1}" class="${key === stage ? 'active' : stageCompleted(key) ? 'done' : ''}"><strong>${title}</strong><small>${description}</small></li>`).join('');
  const running = p.status.startsWith('running:'); const error = p.status === 'interrupted' || p.status === 'needs_human';
  $('#status-banner').className = `status-banner ${running ? 'running ' : ''}${error ? 'error' : ''}`;
  $('#status-banner').innerHTML = `<span>${running ? '◌' : error ? '!' : '●'}</span><span>${esc(p.error || (p.artifacts.review?.content?.summary?.includes('本地模拟审核') ? '模拟审核 · 仅供验收' : statusText[p.status]) || `正在执行 ${p.status.slice(8)}`)}</span>${running ? `<span id="run-elapsed" class="run-elapsed"></span><button class="secondary small" id="cancel-run">取消</button>` : error && recoverStage() ? '<button class="secondary small" id="recover-run">从当前阶段恢复</button>' : ''}`;
  if (running) {
    const startEvt = state.detail.events.find((e) => e.kind === 'stage' && e.message.startsWith('开始执行'));
    state.runningSince = startEvt ? new Date(startEvt.created_at).getTime() : Date.now();
    startRunAnimation(role);
  } else stopRunAnimation();
  const autorunButton = $('#autorun-toggle'); autorunButton.textContent = `全自动：${state.detail.autorun ? '开' : '关'}`; autorunButton.classList.toggle('active', !!state.detail.autorun);
  if ($('#cancel-run')) $('#cancel-run').onclick = () => action(`/api/projects/${p.id}/cancel`, '已请求取消；已发出的外部请求不会被重复提交');
  if ($('#recover-run')) $('#recover-run').onclick = () => action(`/api/projects/${p.id}/run/${recoverStage()}`, '正在恢复当前阶段');
  $('#export-project').disabled = p.status !== 'approved'; $('#asset-count').textContent = `(${state.detail.assets.length})`;
  renderArtifact(); renderAssets(); renderHistory();
  if (!state.editing) loadPreview();
}
function recoverStage() { const p = state.project; if (p.status === 'needs_human') return 'revise'; if (!p.artifacts.research) return 'research'; if (p.artifacts.research && !p.artifacts.research.approved) return null; if (!p.artifacts.framework) return 'framework'; if (!p.artifacts.framework.approved) return null; if (!p.artifacts.document) return 'document'; if (!p.artifacts.interaction) return 'interaction'; if (!p.artifacts.resources) return 'resources'; return 'review'; }
function renderArtifact() {
  const p = state.project; const stage = currentStage();
  let key = stage;
  if (p.status === 'revision_review') key = 'revision_plan';
  if (stage === 'document' || stage === 'interaction' || stage === 'resources') key = 'document';
  const record = p.artifacts[key];
  if (!record) { $('#tab-deliverable').innerHTML = `<div class="empty-projects"><span>◇</span><h3>${p.status.startsWith('running:') ? '正在整理阶段产物' : '此阶段尚未开始'}</h3><p>${p.status.startsWith('running:') ? '工具调用和模型输出会写入日志。完成后请审阅，不会自动越过人工门禁。' : '完成前置阶段后，这里将展示产物。'}</p></div>`; return; }
  const content = record.content;
  let html = `<div class="artifact-title"><h2>${key === 'research' ? '调研与撰写意见' : key === 'framework' ? '文档框架' : key === 'review' ? '架构审核报告' : key === 'revision_plan' ? '修改影响范围' : '当前正文'}</h2><small>版本 ${record.version}${record.approved ? ' · 已确认' : ''}</small></div>`;
  if (key === 'research') html += researchHtml(content);
  else if (key === 'framework') html += frameworkHtml(content);
  else if (key === 'review') html += reviewHtml(content);
  else if (key === 'revision_plan') html += `<div class="artifact-section"><h3>${content.research_needed ? '需要重新调研' : '沿用已确认调研'}</h3><p>${esc(content.reason)}</p></div><div class="artifact-section"><h3>受影响章节</h3><ul>${(content.affected_sections || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div>`;
  else { html += `<div class="artifact-section"><p>正文已完成当前阶段处理。请在“文档预览”中查看图文内容，或在下方查看 Markdown 源稿。</p><textarea class="json-editor" readonly>${esc(content.markdown)}</textarea></div>`; }
  if (['research', 'framework', 'revision_plan'].includes(key) && ['research_review', 'framework_review', 'revision_review'].includes(p.status)) {
    html += `<div class="gate-form"><label for="gate-feedback">补充或修改意见</label><textarea id="gate-feedback" placeholder="写下需要补充、调整或删除的内容；Agent 将继续分析，直至你确认。"></textarea><div class="gate-actions"><button class="secondary" id="manual-gate">直接编辑 JSON</button><button class="secondary" id="send-feedback">提交反馈并重做</button><button class="primary" id="approve-gate">确认并进入下一阶段 →</button></div></div>`;
  }
  if (key === 'review' && !content.approved) {
    html += `<div class="gate-actions"><button class="primary" id="retry-review">${p.review_round >= 3 ? '审核未通过 · 已达 3 轮' : '按审核意见修订并复审'}</button></div>`;
  }
  $('#tab-deliverable').innerHTML = html;
  if ($('#approve-gate')) $('#approve-gate').onclick = () => action(`/api/projects/${p.id}/stages/${key}/approve`, '已确认，正在进入下一阶段');
  if ($('#send-feedback')) $('#send-feedback').onclick = () => { const feedback = $('#gate-feedback').value.trim(); if (!feedback) return toast('请先填写反馈'); action(`/api/projects/${p.id}/stages/${key}/feedback`, '正在按反馈重做', { feedback }); };
  if ($('#manual-gate')) $('#manual-gate').onclick = () => editGate(key, content);
  if ($('#retry-review')) $('#retry-review').onclick = () => action(`/api/projects/${p.id}/run/revise`, '正在分派审核问题并修订');
}
function researchHtml(data) {
  return `<div class="artifact-section"><h3>方向判断</h3><p>${esc(data.summary)}</p></div><h3>参考对象分析</h3>${(data.findings || []).map((f) => `<div class="finding"><strong>${esc(f.topic)}</strong><small> · ${esc(f.confidence)} · 证据 ${(f.evidence_ids || []).map(esc).join(', ') || '无'}</small><p>${esc(f.observation)}</p><p><b>设计启示：</b>${esc(f.implication)}</p></div>`).join('')}<div class="artifact-section"><h3>具体撰写意见</h3><ul>${(data.writing_advice || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div><div class="artifact-section"><h3>设计建议</h3><ul>${(data.recommendations || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div>${(data.limitations || []).length ? `<div class="notice"><b>证据边界：</b>${data.limitations.map(esc).join('；')}</div>` : ''}`;
}
function frameworkHtml(data) { return `<div class="artifact-section"><p>${esc(data.summary)}</p></div>${(data.sections || []).map((s, i) => `<div class="finding"><strong>${i+1}. ${esc(s.title)}</strong><p>${esc(s.purpose)}</p><ul>${s.requirements.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>${s.visuals.length ? `<small>图示：${s.visuals.map(esc).join('；')}</small>` : ''}</div>`).join('')}`; }
function reviewHtml(data) { return `<div class="notice"><b>${data.approved ? '✓ 审核通过' : '审核未通过'}</b><br>${esc(data.summary)}</div>${(data.findings || []).map((f) => `<div class="review-item"><strong>${esc(f.section)} · ${esc(f.severity)} · ${roles[f.owner]}</strong><p>${esc(f.problem)}</p><p>修订建议：${esc(f.recommendation)}</p></div>`).join('')}`; }
function editGate(stage, content) {
  openModal(`<h2 id="modal-title">手动编辑阶段产物</h2><p class="modal-intro">保存前会进行结构校验；保存后仍需由你明确确认。</p><textarea id="gate-json" class="json-editor">${esc(JSON.stringify(content, null, 2))}</textarea><div class="form-actions"><button class="secondary" data-close>取消</button><button class="primary" id="save-gate">保存修改</button></div>`, true);
  $('#save-gate').onclick = () => action(`/api/projects/${state.project.id}/stages/${stage}/feedback`, '已保存手动修改', { content: $('#gate-json').value, feedback: '用户手动编辑阶段产物' }, true);
}
function renderAssets() {
  $('#assets-list').innerHTML = state.detail.assets.length ? `<div class="asset-grid">${state.detail.assets.map((a) => `<div class="asset-card"><img src="/api/projects/${state.project.id}/asset?path=${encodeURIComponent(a.local_path)}" alt="项目素材"><p>${esc(a.metadata.width)} × ${esc(a.metadata.height)} · ${esc(a.local_path)}</p><p>原图：<a href="${esc(a.source_url)}" target="_blank" rel="noopener noreferrer">${esc(a.source_url)}</a></p>${a.source_page ? `<p>来源页：${esc(a.source_page)}</p>` : ''}</div>`).join('')}</div>` : '<p class="modal-intro">尚无已下载素材。检索到的候选不会自动算作项目素材。</p>';
  const evidence = state.detail.evidence.flatMap((e) => (e.payload.results || []).map((r) => ({ ...r, query: e.query })));
  $('#evidence-list').innerHTML = evidence.length ? evidence.map((r) => `<div class="evidence"><small>${esc(r.id || '')} · ${esc(r.site_name || '检索记录')}</small><br><a href="${esc(r.url)}" target="_blank" rel="noopener noreferrer">${esc(r.title || r.url)}</a><p>${esc(r.snippet || r.summary || '')}</p><small>检索词：${esc(r.query)}</small></div>`).join('') : '<p class="modal-intro">尚无检索记录。</p>';
}
function renderHistory() {
  $('#versions-list').innerHTML = state.detail.versions.length ? state.detail.versions.slice().reverse().map((v) => `<div class="evidence"><b>${esc(v.stage)} · v${v.version}</b><small> ${esc(v.created_at)}</small><p>${esc(v.feedback || 'Agent 生成')}</p><details><summary>查看该版本</summary><pre>${esc(JSON.stringify(v.content, null, 2))}</pre></details></div>`).join('') : '<p>暂无版本记录</p>';
  $('#event-list').innerHTML = state.detail.events.map((e) => `<div class="event"><small>${esc(e.created_at.slice(0, 19).replace('T', ' '))}</small>${esc(e.message)}</div>`).join('');
}
async function loadPreview() { if (!state.project) return; try { const data = await api(`/api/projects/${state.project.id}/preview`); $('#document-preview').innerHTML = data.html || '<p class="modal-intro">正文将在框架确认后生成。</p>'; $('#document-editor').value = data.markdown; } catch (_) {} }
async function action(path, message, body = null, close = false) {
  try { await api(path, { method: body ? 'POST' : 'POST', body: body ? JSON.stringify(body) : undefined }); if (close) closeModal(); toast(message); await refreshProjects(); if (state.project) await loadProject(state.project.id); } catch (error) { toast(error.message); if (state.project) await loadProject(state.project.id); }
}
async function refreshProjects() { const data = await api('/api/bootstrap'); state.csrf = data.csrf_token; state.projects = data.projects; state.settings = data.roles; renderProjects(); }
function settingsModal(role = state.role) {
  state.role = role; const s = state.settings[role];
  openModal(`<h2 id="modal-title">Agent 与搜索设置</h2><p class="modal-intro">每个角色独立配置。更改只影响后续执行；已保存的阶段快照不会被改写。</p><div class="settings-layout"><div class="role-list">${Object.entries(roles).map(([key, value]) => `<button data-role="${key}" class="${key === role ? 'active' : ''}">${value}</button>`).join('')}</div><form id="settings-form" class="setting-form"><div class="field-row"><label class="field"><span>模型</span><input name="model" value="${esc(s.model)}" required></label><label class="field"><span>Base URL</span><input name="base_url" value="${esc(s.base_url)}" required></label></div><div class="field-row"><label class="field"><span>温度</span><input name="temperature" type="number" min="0" max="2" step="0.1" value="${s.temperature}"></label><label class="field"><span>最大输出 Tokens</span><input name="max_tokens" type="number" min="512" max="65536" value="${s.max_tokens}"></label></div><label class="field"><span>密钥环境变量名</span><input name="api_key_env" value="${esc(s.api_key_env)}"><small>显式保存的本地密钥优先；未保存时读取此环境变量。</small></label>${role === 'writer' ? `<label class="field"><span>视觉标注模型</span><input name="vision_model" value="${esc(s.vision_model || 'deepseek-flash')}"><small>仅用于观察已下载参考图并选择标注位置。</small></label>` : ''}<label class="field"><span>API Key ${s.key_configured ? '（已配置，留空保持不变）' : '（待配置）'}</span><input name="api_key" type="password" autocomplete="off" placeholder="不会回显、不会导出"></label><label class="field"><span>Skill 名称（逗号分隔）</span><input name="skills" value="${esc(s.skills.join(', '))}"></label><label class="field"><span>系统提示词</span><textarea name="prompt">${esc(s.prompt)}</textarea></label><div class="form-actions"><button class="primary" type="submit">保存此角色</button></div></form></div><div class="settings-footer"><h3>豆包图片搜索</h3><form id="search-key-form" class="field-row"><label class="field"><span>WEB_SEARCH_API_KEY</span><input name="api_key" type="password" placeholder="图片搜索专用密钥" autocomplete="off"></label><div class="form-actions"><button class="secondary" type="submit">保存搜索密钥</button></div></form><div class="notice">DeepSeek 用于模型、网页检索与抓取；豆包用于图片检索。图片仅作研究参考，请核对使用授权。参考图标注使用 writer 的 vision_model（默认 deepseek-flash）。</div></div>`, true);
  document.querySelectorAll('[data-role]').forEach((b) => b.onclick = () => settingsModal(b.dataset.role));
  $('#settings-form').onsubmit = async (event) => { event.preventDefault(); const data = Object.fromEntries(new FormData(event.target)); data.temperature = Number(data.temperature); data.max_tokens = Number(data.max_tokens); data.skills = data.skills.split(',').map((x) => x.trim()).filter(Boolean); if (!data.api_key) delete data.api_key; try { await api(`/api/settings/${role}`, { method: 'PUT', body: JSON.stringify(data) }); toast('角色设置已保存'); await refreshProjects(); settingsModal(role); } catch (error) { toast(error.message); } };
  $('#search-key-form').onsubmit = async (event) => { event.preventDefault(); const api_key = new FormData(event.target).get('api_key'); try { await api('/api/settings/search/key', { method: 'PUT', body: JSON.stringify({ api_key }) }); toast('图片搜索密钥已保存'); event.target.reset(); } catch (error) { toast(error.message); } };
}
function revisionModal() {
  openModal(`<h2 id="modal-title">修改现有文档</h2><p class="modal-intro">保留当前版本，先由架构策划分析修改范围，经你确认后才重做受影响阶段。</p><form id="revision-form"><label class="field"><span>修改目的 *</span><textarea name="purpose" required placeholder="希望解决什么问题，达到什么效果"></textarea></label><label class="field"><span>修改范围</span><textarea name="scope" placeholder="指定章节、系统或交互；留空由架构策划判断"></textarea></label><label class="field"><span>保留约束</span><textarea name="constraints" placeholder="哪些规则、结构、素材或表达不得改变"></textarea></label><div class="form-actions"><button class="secondary" type="button" data-close>取消</button><button class="primary" type="submit">分析影响范围 →</button></div></form>`);
  $('#revision-form').onsubmit = (event) => { event.preventDefault(); action(`/api/projects/${state.project.id}/revision`, '正在分析修改影响范围', Object.fromEntries(new FormData(event.target)), true); };
}
function exportModal() {
  openModal(`<h2 id="modal-title">交付文档与资料</h2><p class="modal-intro">将定稿 Markdown、DOCX、检索证据、图片与图示、审核历史和来源清单一并整理。审核通过才会生成定稿。</p><div class="notice">参考图片用于设计研究，可检索不代表获得再发布授权。交付前请核查来源与使用范围。</div><div class="form-actions"><button class="primary" id="generate-export">生成并整理交付包</button></div><div id="download-links"></div>`);
  $('#generate-export').onclick = async () => { const button = $('#generate-export'); button.disabled = true; button.textContent = '正在生成 DOCX 与归档…'; try { const data = await api(`/api/projects/${state.project.id}/export`); $('#download-links').innerHTML = `<p class="modal-intro">已保存至：${esc(data.directory)}</p><div class="form-actions"><a class="secondary" href="/api/projects/${state.project.id}/download/markdown">Markdown</a><a class="secondary" href="/api/projects/${state.project.id}/download/docx">DOCX</a><a class="primary" href="/api/projects/${state.project.id}/download/zip">完整资料 ZIP</a></div>`; button.hidden = true; } catch (error) { toast(error.message); button.disabled = false; button.textContent = '生成并整理交付包'; } };
}
$('#new-project').onclick = newProject; $('.brand').onclick = (e) => { e.preventDefault(); showHome(); }; $('#back-home').onclick = showHome; $('#open-settings').onclick = () => settingsModal(); $('#revise-project').onclick = revisionModal; $('#export-project').onclick = exportModal;
$('#autorun-toggle').onclick = async () => {
  const p = state.project; if (!p) return;
  const enable = !(state.detail && state.detail.autorun);
  try {
    await api(`/api/projects/${p.id}/autorun`, { method: 'PUT', body: JSON.stringify({ enabled: enable }) });
    toast(enable ? '已开启全自动流程：每步完成后自动确认进入下一步' : '已关闭全自动流程');
    await refreshProjects(); await loadProject(p.id);
    if (!enable) return;
    const cur = state.project;
    const gate = { research_review: 'research', framework_review: 'framework', revision_review: 'revision_plan' }[cur.status];
    if (gate) action(`/api/projects/${cur.id}/stages/${gate}/approve`, '全自动：已确认，进入下一阶段');
    else if (['interaction', 'resources'].includes(cur.status)) action(`/api/projects/${cur.id}/run/${cur.status}`, '全自动：继续下一阶段');
    else if (cur.status === 'review' && cur.artifacts.review && !cur.artifacts.review.content.approved) action(`/api/projects/${cur.id}/run/revise`, '全自动：按审核意见修订并复审');
    else if (cur.status === 'review') action(`/api/projects/${cur.id}/run/review`, '全自动：开始架构审核');
  } catch (error) { toast(error.message); }
};
$('.modal-close').onclick = closeModal; $('#modal-backdrop').addEventListener('click', (e) => { if (e.target === $('#modal-backdrop')) closeModal(); }); document.addEventListener('click', (e) => { if (e.target.matches('[data-close]')) closeModal(); }); document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeModal(); });
document.querySelectorAll('[data-tab]').forEach((button) => button.onclick = () => { document.querySelectorAll('[data-tab]').forEach((b) => b.classList.toggle('active', b === button)); document.querySelectorAll('.tab-content').forEach((el) => el.hidden = el.id !== `tab-${button.dataset.tab}`); if (button.dataset.tab === 'document') loadPreview(); });
$('#edit-document').onclick = async () => { if (!state.editing) { state.editing = true; $('#document-preview').hidden = true; $('#document-editor').hidden = false; $('#edit-document').textContent = '保存并重新进入资源阶段'; } else { try { await api(`/api/projects/${state.project.id}/document`, { method: 'PUT', body: JSON.stringify({ content: $('#document-editor').value }) }); state.editing = false; $('#document-preview').hidden = false; $('#document-editor').hidden = true; $('#edit-document').textContent = '编辑 Markdown'; toast('正文已保存；请继续资源拆解与审核'); await loadProject(state.project.id); } catch (error) { toast(error.message); } } };
refreshProjects().catch((error) => toast(error.message));
