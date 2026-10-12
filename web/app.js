const invoke=window.__TAURI__.core.invoke;

const { languages, i18n } = window.BaldI18n;

let state=null,windows=[],language="korean",theme="system",elevationRequested=false;

let refreshRequest=null;

let initialized=false;

let uiVisible=false,refreshTimer=null,visibilityGeneration=0,elevationRetry=false;

const $=s=>document.querySelector(s);

let renderedApplicationsHtml="";

function renderDragControl(rule, translations) {
  const mode = rule.drag_mode === "enabled" ? "enabled" : "disabled";

  const label = mode === "enabled" ? translations.dragAllowedLabel : translations.dragBlockedLabel;

  return `<div class="dropdown-wrap app-drag-control" id="drag-${rule.id}">
    <button class="dropdown-trigger" data-drag-trigger aria-label="${translations.windowDrag}" aria-haspopup="menu" aria-expanded="false"><span>${label}</span><svg class="chevron" width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M5 7.5L10 12.5L15 7.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
    <div class="dropdown-menu" role="menu">
      <button class="dropdown-item${mode === "enabled" ? " is-selected" : ""}" data-drag-rule="${rule.id}" data-drag-mode="enabled" role="menuitemradio" aria-checked="${mode === "enabled"}">${translations.dragEnabled}<img class="check-icon" src="figma/check.svg" alt=""></button>
      <button class="dropdown-item${mode === "disabled" ? " is-selected" : ""}" data-drag-rule="${rule.id}" data-drag-mode="disabled" role="menuitemradio" aria-checked="${mode === "disabled"}">${translations.dragDisabled}<img class="check-icon" src="figma/check.svg" alt=""></button>
    </div>
  </div>`;
}

async function changeDragMode(button) {
  button.disabled = true;
  closeMenus();

  try {
    await invoke("set_application_drag_mode", { id: button.dataset.dragRule, mode: button.dataset.dragMode });
  } catch (error) {
    reportWindowError(error);
  } finally {
    button.disabled = false;
    await refresh(true);
  }
}

function esc(v){const e=document.createElement("span");e.textContent=v??"";

return e.innerHTML}

function applyTheme(){document.documentElement.dataset.theme=theme;$("#themeValue").textContent=i18n[language][theme==="light"?"themeLight":theme==="dark"?"themeDark":"themeSystem"];document.querySelectorAll("[data-theme]").forEach(e=>e.classList.toggle("is-selected",e.dataset.theme===theme))}

function renderLanguageMenu() {
  const menu = $("#languageDropdown .dropdown-menu");
  menu.innerHTML = "";

  for (const [key, locale] of Object.entries(languages)) {
    menu.innerHTML += `<button class="dropdown-item" data-language="${key}"><span lang="${locale.tag}">${locale.name}</span><img class="check-icon" src="figma/check.svg" alt=""></button>`;
  }
}

function applyLanguage() {
  const t = i18n[language];
  document.documentElement.lang = languages[language].tag;
  document.querySelectorAll("[data-i18n]").forEach(element => element.textContent = t[element.dataset.i18n]);
  document.querySelectorAll("[data-i18n-placeholder]").forEach(element => element.placeholder = t[element.dataset.i18nPlaceholder]);
  document.querySelectorAll("[data-i18n-label]").forEach(element => element.setAttribute("aria-label", t[element.dataset.i18nLabel]));
  const updateButton = $("#btnCheckUpdate");

  if (updateButton.disabled) updateButton.textContent = t.checking;

  const preference = state.config.language;
  $("#languageValue").textContent = languages[language].name;
  document.querySelectorAll("[data-language]").forEach(element => {
    const selected = element.dataset.language === preference;
    element.classList.toggle("is-selected", selected);
    element.setAttribute("aria-pressed", String(selected));
  });

  applyTheme();

  if ($("#picker").classList.contains("is-open")) renderPicker();
}

function applicationIcon(icon){return icon?`<img class="app-icon" src="${icon}" alt="">`:'<span class="app-icon-placeholder"></span>'}

function findLiveIcon(rule){const path=rule.executable_path?.toLowerCase();

return windows.find(w=>path&&w.executablePath?.toLowerCase()===path)?.icon||null}

function waitForMotion(ms){return matchMedia("(prefers-reduced-motion: reduce)").matches?Promise.resolve():new Promise(resolve=>setTimeout(resolve,ms))}

let toastTimer=null;

function showToast(message,duration=3000){const toast=$("#toast");toast.textContent=message;toast.classList.add("is-visible");clearTimeout(toastTimer);toastTimer=setTimeout(()=>toast.classList.remove("is-visible"),duration)}

function reportWindowError(error){showToast(`${i18n[language].windowError}:\n${error}`,7000)}

async function runConfigCommand(command,args){
  try{await invoke(command,args)}catch(error){reportWindowError(error)}finally{await refresh(true)}
}

async function checkForUpdates(){const button=$("#btnCheckUpdate"),t=i18n[language];

if(button.disabled)return;button.disabled=true;button.textContent=t.checking;

try{const result=await invoke("check_for_updates_manual");const current=i18n[language];showToast(result==="LATEST"?current.latest:result==="BUSY"?current.installing:result==="INSTALLED"?current.installed:result,3000)}catch(error){showToast(`${i18n[language].updateFailed}:\n${error}`,3000)}finally{button.disabled=false;button.textContent=i18n[language].checkUpdate}}

const resetModal=$("#resetModal"),resetModalCard=resetModal.querySelector(".t-modal");

let resetCloseTimer=null,resetDimTimer=null;

function openResetModal(){clearTimeout(resetCloseTimer);clearTimeout(resetDimTimer);resetModalCard.classList.remove("is-closing");$("#dim").classList.add("is-open","is-reset");resetModal.classList.add("is-open");resetModalCard.classList.add("is-open");resetModal.setAttribute("aria-hidden","false");$("#resetCancel").focus({preventScroll:true})}

function closeResetModal(){const dim=$("#dim");dim.classList.remove("is-open");resetModal.classList.remove("is-open");resetModalCard.classList.remove("is-open");resetModalCard.classList.add("is-closing");resetModal.setAttribute("aria-hidden","true");const closeMs=parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--modal-close-dur"))||150;clearTimeout(resetCloseTimer);clearTimeout(resetDimTimer);resetCloseTimer=setTimeout(()=>resetModalCard.classList.remove("is-closing"),closeMs);resetDimTimer=setTimeout(()=>dim.classList.remove("is-reset"),260)}

async function removeHomeApplication(button){if(button.dataset.pending)return;button.dataset.pending="true";const row=button.closest(".app-row"),divider=row.previousElementSibling?.classList.contains("divider")?row.previousElementSibling:null;row.classList.add("is-removing");divider?.classList.add("is-removing");

try{await waitForMotion(300);await invoke("remove_application",{id:button.dataset.remove});await refresh(true)}catch(error){row.classList.remove("is-removing");divider?.classList.remove("is-removing");delete button.dataset.pending;reportWindowError(error);await refresh(true)}}

async function renderApplications() {
  const t = i18n[language], rules = state.config.applications;

  let html = "";

  for (const [index, rule] of rules.entries()) {
    if (index) html += '<div class="divider inset"></div>';

    const icon = state.applicationIcons?.[rule.id] || findLiveIcon(rule);

    html += `<div class="app-row">${applicationIcon(icon)}<div class="app-copy"><strong>${esc(rule.display_name)}</strong>${renderDragControl(rule, t)}</div><button class="remove-btn" data-remove="${rule.id}" aria-label="${t.remove}"><img src="figma/remove-x.svg" alt=""></button></div>`;
  }

  if (rules.length) html += '<div class="divider"></div>';

  html += `<button class="add-app-btn" id="addApplication"><span class="add-app-btn-content"><img src="figma/plus.svg" alt=""><span>${t.addApplication}</span></span></button>`;

  if (html !== renderedApplicationsHtml) {
    closeMenus();
    renderedApplicationsHtml = html;
    $("#applications").innerHTML = html;
    document.querySelectorAll("[data-remove]").forEach(button => button.onclick = event => {
      event.stopPropagation();

      removeHomeApplication(button);
    });
    document.querySelectorAll("[data-drag-trigger]").forEach(button => button.onclick = event => {
      event.stopPropagation();

      toggleDropdown(button.closest(".dropdown-wrap"));
    });
    document.querySelectorAll("[data-drag-mode]").forEach(button => button.onclick = event => {
      event.stopPropagation();

      changeDragMode(button);
    });
    $("#addApplication").onclick = openPicker;
  }
}

function pickerActionIcon(added){return `<span class="t-icon-swap picker-action-icon" data-state="${added?"b":"a"}" aria-hidden="true"><span class="t-icon" data-icon="a"><svg viewBox="0 0 20 20"><path d="M10 3V17M3 10H17"/></svg></span><span class="t-icon t-success-check" data-icon="b"><svg viewBox="0 0 16 16"><path d="M3 8L6.5 11.5L13 4.5"/></svg></span></span>`}

function applicationKey(value){return (value.executable_path||value.executablePath||value.executable_name||value.executableName||"").toLowerCase()}

function findAddedRule(choice){const key=applicationKey(choice);

return state.config.applications.find(rule=>applicationKey(rule)===key)}

async function swapPickerText(button,next){const element=button.querySelector(".t-text-swap");

if(element.textContent===next)return;const reduced=matchMedia("(prefers-reduced-motion: reduce)").matches,textDuration=reduced?0:150,resizeDuration=reduced?0:300,startWidth=button.offsetWidth;button.style.width=`${startWidth}px`;element.classList.add("is-exit");await waitForMotion(textDuration);element.textContent=next;element.classList.remove("is-exit");element.classList.add("is-enter-start");button.style.width="auto";const targetWidth=button.offsetWidth;button.style.width=`${startWidth}px`;void button.offsetWidth;button.style.width=`${targetWidth}px`;void element.offsetHeight;element.classList.remove("is-enter-start");await waitForMotion(resizeDuration);button.style.width=""}

async function togglePickerApplication(button,choice){if(button.dataset.pending)return;button.dataset.pending="true";const wasAdded=button.classList.contains("is-added"),rule=findAddedRule(choice);

try{if(wasAdded&&rule)await invoke("remove_application",{id:rule.id});else if(!wasAdded)await invoke("add_application",{choice});

if(!await refresh(true))return;const isAdded=Boolean(findAddedRule(choice));button.classList.toggle("is-added",isAdded);button.querySelector(".t-icon-swap").dataset.state=isAdded?"b":"a";await swapPickerText(button,i18n[language][isAdded?"added":"add"]);await renderApplications()}catch(error){reportWindowError(error)}finally{delete button.dataset.pending}}

function renderPicker(){const t=i18n[language],query=$("#pickerSearch").value.trim().toLowerCase(),visible=windows.filter(w=>!query||w.title.toLowerCase().includes(query)||w.executableName.toLowerCase().includes(query));$("#pickerList").innerHTML=visible.map((w,index)=>{const added=Boolean(findAddedRule(w));

return `<div class="picker-row">${applicationIcon(w.icon)}<div class="app-copy"><strong>${esc(w.title)}</strong><span>${esc(w.executableName)}</span></div><button class="add-one-btn t-resize${added?" is-added":""}" data-index="${index}">${pickerActionIcon(added)}<span class="t-text-swap picker-action-label">${added?t.added:t.add}</span></button></div>`}).join("")||`<div class="picker-empty">${t.empty}</div>`;document.querySelectorAll(".add-one-btn").forEach(button=>button.onclick=()=>togglePickerApplication(button,visible[Number(button.dataset.index)]))}

async function openPicker(){try{windows=await invoke("list_windows");$("#pickerSearch").value="";renderPicker();$("#dim").classList.add("is-open");$("#picker").classList.add("is-open");$("#picker").setAttribute("aria-hidden","false");setTimeout(()=>{if($("#picker").classList.contains("is-open"))$("#pickerSearch").focus()},360)}catch(error){reportWindowError(error)}}

function clearPickerDrag(){
  cancelAnimationFrame(pickerMotion.frame);
  pickerMotion.frame=null;

  if(pickerDrag.active){
    const head=$(".picker-head");

    if(head.hasPointerCapture(pickerDrag.pointerId))head.releasePointerCapture(pickerDrag.pointerId);
  }

  const picker=$("#picker"),dim=$("#dim");
  pickerDrag.active=false;
  picker.classList.remove("is-dragging");
  picker.style.removeProperty("--picker-drag-y");
  dim.classList.remove("is-dragging");
  dim.style.opacity="";
}

function closePicker(){clearPickerDrag();$("#dim").classList.remove("is-open");$("#picker").classList.remove("is-open");$("#picker").setAttribute("aria-hidden","true")}

function refresh(afterChange=false){
  if(refreshRequest)return afterChange?refreshRequest.then(()=>refresh()):refreshRequest;
  refreshRequest=readState().catch(error=>{reportWindowError(error);

return null}).finally(()=>{refreshRequest=null});

  return refreshRequest;
}

async function readState(){const first=!state,generation=visibilityGeneration,next=await invoke("get_state",{knownRevision:state?.revision});

const newStartupError=next.startupError&&next.startupError!==state?.startupError;

if(next.config){state=next;language=state.config.language;theme=state.config.theme||"system";applyLanguage();$("#automaticToggle").checked=state.config.automatic_application;$("#startupToggle").checked=state.startupEnabled;await renderApplications()}else if(state){state={...state,...next}}else{throw new Error("Initial settings are missing")}

if(newStartupError)reportWindowError(next.startupError);

if(generation===visibilityGeneration)setUiVisibility(next.uiVisible,false);

if(!elevationRequested&&Object.values(state.statuses||{}).includes("requires_elevation")){elevationRequested=true;invoke("elevate_border_service",{retry:elevationRetry}).catch(reportWindowError);elevationRetry=false}

if(first&&initialized)revealContent().catch(reportWindowError);

return state}

function setUiVisibility(visible,immediate=true){
  if(uiVisible===visible)return;
  uiVisible=visible;visibilityGeneration++;clearTimeout(refreshTimer);refreshTimer=null;

  if(visible){if(immediate)refresh().finally(scheduleRefresh);else scheduleRefresh()}
}

function scheduleRefresh(){
  clearTimeout(refreshTimer);refreshTimer=null;

  if(!uiVisible)return;
  refreshTimer=setTimeout(async()=>{refreshTimer=null;

    try{if(uiVisible&&!$("#picker").classList.contains("is-open"))await refresh()}finally{scheduleRefresh()}
  },2000);
}

let openDD=null;

function closeMenus() {
  document.querySelectorAll(".dropdown-wrap.is-open").forEach(wrap => {
    wrap.classList.remove("is-open");
    wrap.querySelector(".dropdown-menu").classList.remove("is-open");
    wrap.querySelector(".dropdown-trigger").setAttribute("aria-expanded", "false");
  });
  $("#overlay").classList.remove("is-open");
  openDD = null;
}

function toggleDropdown(wrap) {
  if (openDD && openDD !== wrap.id) closeMenus();

  const menu = wrap.querySelector(".dropdown-menu"), trigger = wrap.querySelector(".dropdown-trigger");
  const open = !wrap.classList.contains("is-open");

  if (open) {
    if (wrap.id === "languageDropdown" && !menu.children.length) {
      renderLanguageMenu();
      applyLanguage();
    }

    const r = trigger.getBoundingClientRect(), appR = $(".app").getBoundingClientRect();
    const pagePadding = parseFloat(getComputedStyle($(".main")).paddingLeft);
    const minTop = $(".header").getBoundingClientRect().bottom + pagePadding;
    const maxBottom = appR.bottom - pagePadding;
    const belowTop = Math.max(r.bottom, minTop) + 8;
    const aboveBottom = Math.min(r.top, maxBottom) - 8;
    const itemCount = menu.querySelectorAll(".dropdown-item").length;
    const menuHeight = itemCount * 42 + 8, spaceBelow = maxBottom - belowTop;
    const spaceAbove = aboveBottom - minTop;
    const opensUp = spaceBelow < menuHeight && spaceAbove > spaceBelow;
    menu.style.maxHeight = `${Math.max(0, opensUp ? spaceAbove : spaceBelow)}px`;

    menu.classList.toggle("opens-up", opensUp);

    if (wrap.classList.contains("app-drag-control")) {
      menu.style.left = `${Math.max(8, Math.min(r.left, innerWidth - menu.offsetWidth - 8))}px`;
      menu.style.right = "auto";
    } else {
      menu.style.left = "auto";
      menu.style.right = `${innerWidth - appR.right + 20}px`;
    }

    menu.style.top = opensUp ? "auto" : `${belowTop}px`;
    menu.style.bottom = opensUp ? `${innerHeight - aboveBottom}px` : "auto";
  }

  wrap.classList.toggle("is-open", open);
  menu.classList.toggle("is-open", open);
  trigger.setAttribute("aria-expanded", String(open));
  $("#overlay").classList.toggle("is-open", open);
  openDD = open ? wrap.id : null;
}

const pickerDrag={active:false,pointerId:null,startY:0,lastY:0,lastTime:0,velocity:0,dy:0};

const pickerMotion={frame:null,y:0,velocity:0};

function paintPickerDrag(){
  const picker=$("#picker"),distance=picker.offsetHeight+72;
  picker.style.setProperty("--picker-drag-y",`${pickerMotion.y}px`);
  $("#dim").style.opacity=String(Math.max(0,Math.min(1,1-pickerMotion.y/distance)));
}

function beginPickerDrag(e){
  const picker=$("#picker");

  if(e.button!==0||pickerDrag.active||e.target.closest("button")||!picker.classList.contains("is-open"))return;

  pickerMotion.y=new DOMMatrixReadOnly(getComputedStyle(picker).transform).m42;
  pickerMotion.velocity=0;
  cancelAnimationFrame(pickerMotion.frame);
  pickerMotion.frame=null;
  // Recover the input offset from the resisted position when catching a return.
  const offset=pickerMotion.y<0?-Math.pow(-pickerMotion.y,1/.65):pickerMotion.y;
  pickerDrag.active=true;
  pickerDrag.pointerId=e.pointerId;
  pickerDrag.startY=e.clientY-offset;
  pickerDrag.lastY=e.clientY;
  pickerDrag.lastTime=performance.now();
  pickerDrag.velocity=0;
  pickerDrag.dy=offset;
  picker.classList.add("is-dragging");
  $("#dim").classList.add("is-open","is-dragging");
  paintPickerDrag();
  e.currentTarget.setPointerCapture(e.pointerId);
}

function movePickerDrag(e){
  if(!pickerDrag.active||e.pointerId!==pickerDrag.pointerId)return;

  e.preventDefault();
  const now=performance.now(),elapsed=Math.max(1,now-pickerDrag.lastTime),raw=e.clientY-pickerDrag.startY;
  const y=raw<0?-Math.min(14,Math.pow(-raw,.65)):raw;
  pickerMotion.velocity=(y-pickerMotion.y)/elapsed*1000;
  pickerMotion.y=y;
  pickerDrag.velocity=(e.clientY-pickerDrag.lastY)/elapsed;
  pickerDrag.lastY=e.clientY;
  pickerDrag.lastTime=now;
  pickerDrag.dy=raw;
  paintPickerDrag();
}

function settlePickerDrag(shouldClose){
  const reduced=matchMedia("(prefers-reduced-motion: reduce)");

  if(reduced.matches){
    if(shouldClose)closePicker();else clearPickerDrag();

    return;
  }

  const picker=$("#picker"),target=shouldClose?picker.offsetHeight+72:0;
  const duration=parseFloat(getComputedStyle(document.documentElement).getPropertyValue(shouldClose?"--duration-fast":"--duration-release"));
  const frequency=8/(duration/1000);
  let lastTime=performance.now();

  const advance=now=>{
    if(reduced.matches){
      if(shouldClose)closePicker();else clearPickerDrag();

      return;
    }

    const elapsed=Math.max(0,(now-lastTime)/1000),offset=pickerMotion.y-target;
    const coefficient=pickerMotion.velocity+frequency*offset,decay=Math.exp(-frequency*elapsed);
    // A critically damped return preserves release velocity without oscillation.
    pickerMotion.y=target+(offset+coefficient*elapsed)*decay;
    pickerMotion.velocity=(pickerMotion.velocity-frequency*coefficient*elapsed)*decay;
    lastTime=now;
    paintPickerDrag();

    if(Math.abs(pickerMotion.y-target)<.5&&Math.abs(pickerMotion.velocity)<5){
      if(shouldClose)closePicker();else clearPickerDrag();

      return;
    }

    pickerMotion.frame=requestAnimationFrame(advance);
  };

  pickerMotion.frame=requestAnimationFrame(advance);
}

function endPickerDrag(e){
  if(!pickerDrag.active||e.pointerId!==pickerDrag.pointerId)return;

  pickerDrag.active=false;
  // A held pointer has no remaining throw velocity; cancellation always returns.

  if(e.type==="pointercancel"||performance.now()-pickerDrag.lastTime>80){pickerDrag.velocity=0;pickerMotion.velocity=0}

  const shouldClose=e.type!=="pointercancel"&&(pickerDrag.dy>96||(pickerDrag.dy>36&&pickerDrag.velocity>.55));

  if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);

  settlePickerDrag(shouldClose);
}

$("#pickerClose").onclick=closePicker;

$("#dim").onclick=closePicker;

$("#pickerSearch").oninput=renderPicker;

$("#automaticToggle").onchange=e=>runConfigCommand("set_automatic",{enabled:e.target.checked});

$("#startupToggle").onchange=e=>runConfigCommand("set_startup",{enabled:e.target.checked});

$("#btnCheckUpdate").onclick=checkForUpdates;

$("#resetBtn").onclick=openResetModal;

$("#resetCancel").onclick=closeResetModal;

$("#resetConfirm").onclick=()=>{closeResetModal();

return runConfigCommand("reset_settings")};

$("#exitBtn").onclick=()=>invoke("exit_app").catch(reportWindowError);

const pickerHead=$(".picker-head");

pickerHead.addEventListener("pointerdown",beginPickerDrag);

pickerHead.addEventListener("pointermove",movePickerDrag);

pickerHead.addEventListener("pointerup",endPickerDrag);

pickerHead.addEventListener("pointercancel",endPickerDrag);

document.querySelectorAll(".dropdown-trigger").forEach(button=>button.onclick=e=>{e.stopPropagation();toggleDropdown(button.closest(".dropdown-wrap"))});

$("#languageDropdown .dropdown-menu").addEventListener("click", event => {
  const item = event.target.closest("[data-language]");

  if (!item) return;

  event.stopPropagation();
  closeMenus();
  runConfigCommand("set_language", { language: item.dataset.language });
});

document.querySelectorAll("[data-theme]").forEach(item=>item.onclick=e=>{e.stopPropagation();closeMenus();

return runConfigCommand("set_theme",{theme:item.dataset.theme})});

let contentRevealGeneration = 0;

let cancelContentReveal = null;

function prepareContentReveal() {
  contentRevealGeneration += 1;
  cancelContentReveal?.();
  cancelContentReveal = null;

  const stack = $(".content-stack");

  stack.classList.add("is-preparing");
  stack.classList.remove("is-shown", "is-settled");
  stack.inert = true;
}

async function revealContent() {
  const stack = $(".content-stack");

  // A prepared window can be used immediately when shown again.
  if (stack.classList.contains("is-shown")) {
    contentRevealGeneration += 1;
    cancelContentReveal?.();
    cancelContentReveal = null;
    stack.classList.remove("is-preparing");
    stack.classList.add("is-settled");
    stack.inert = false;

    return;
  }

  prepareContentReveal();

  const generation = contentRevealGeneration;
  const fontFamily = getComputedStyle(document.body).fontFamily;

  // Prepare lazy menu fonts too, so their first opening cannot swap glyph metrics.
  await Promise.allSettled([
    document.fonts.load(`400 17px ${fontFamily}`),
    document.fonts.load(`500 17px ${fontFamily}`),
    document.fonts.load(`600 17px ${fontFamily}`),
    document.fonts.load('300 15px "Pretendard Variable"', "한국어"),
    document.fonts.load('300 15px "Pretendard JP Variable"', "日本語"),
    document.fonts.load('300 15px "Pretendard Std Variable"', "English Русский")
  ]);
  await document.fonts.ready;

  if (generation !== contentRevealGeneration) return;

  const settle = () => {
    stack.classList.remove("is-preparing");
    stack.classList.add("is-shown", "is-settled");
    stack.inert = false;
  };

  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    settle();

    return;
  }

  // Give the browser a painted initial frame before starting the transition.
  requestAnimationFrame(() => requestAnimationFrame(() => {
    if (generation !== contentRevealGeneration) return;

    const finish = event => {
      if (event && (event.target !== stack.lastElementChild || event.propertyName !== "filter")) return;

      cancelContentReveal();
      cancelContentReveal = null;
      settle();
    };

    const style = getComputedStyle(stack);
    const duration = parseFloat(style.getPropertyValue("--stagger-dur"));
    const delay = parseFloat(style.getPropertyValue("--stagger-stagger")) * (stack.children.length - 1);
    const fallback = setTimeout(() => finish(), duration + delay + 80);

    cancelContentReveal = () => {
      clearTimeout(fallback);
      stack.removeEventListener("transitionend", finish);
    };

    stack.addEventListener("transitionend", finish);
    stack.classList.remove("is-preparing");
    stack.classList.add("is-shown");
  }));
}

// Keep content ready while the native window is hidden; Explorer can reopen
// the existing instance without emitting bald-window-reopened.
window.addEventListener("bald-window-reopened",()=>{elevationRequested=false;elevationRetry=true;

if(state)revealContent().catch(reportWindowError);

refresh().catch(reportWindowError)});

function updateHeaderScroll(){
  $(".header").classList.toggle("is-scrolled", $(".main").scrollTop > 0);
}

function updatePickerScroll() {
  $(".picker-surface").classList.toggle("is-scrolled", $("#pickerList").scrollTop > 0);
}

updateHeaderScroll();

updatePickerScroll();

$("#pickerList").addEventListener("scroll", updatePickerScroll, { passive: true });

$("#overlay").addEventListener("click",closeMenus);

window.addEventListener("resize",closeMenus);

$(".main").addEventListener("scroll",()=>{updateHeaderScroll();

if(openDD)closeMenus()},{passive:true});

document.addEventListener("keydown",e=>{if(e.key==="Escape"&&resetModal.classList.contains("is-open")){e.stopPropagation();closeResetModal();

return}

if(e.key==="Escape"){closeMenus();closePicker()}});

const initialVisibilityGeneration=visibilityGeneration;

Promise.all([refresh(),invoke("get_app_version"),invoke("get_ui_visibility")]).then(([,version,visible])=>{$("#appVersionText").textContent=version;

if(initialVisibilityGeneration===visibilityGeneration)setUiVisibility(visible)}).catch(error=>{$("#appVersionText").textContent="-";reportWindowError(error)}).finally(()=>{initialized=true;

if(state)revealContent().catch(reportWindowError)});

window.__TAURI__.event.listen("bald-error",event=>reportWindowError(event.payload)).catch(reportWindowError);

window.__TAURI__.event.listen("bald-ui-visible",event=>setUiVisibility(Boolean(event.payload))).catch(reportWindowError);

$(".header").addEventListener("mousedown",e=>{if(e.button===0&&!e.target.closest("button"))invoke("start_window_drag").catch(reportWindowError)});

// Keep the pressed button as the click target while its scale changes.
function bindWindowAction(button,command){
  button.addEventListener("pointerdown",event=>{
    if(event.button!==0)return;

    event.stopPropagation();
    button.setPointerCapture(event.pointerId);
  });

  button.addEventListener("mousedown",event=>event.stopPropagation());
  button.onclick=()=>invoke(command).catch(reportWindowError);
}

bindWindowAction($("#minimizeBtn"),"minimize_window");

bindWindowAction($("#closeBtn"),"close_window");
