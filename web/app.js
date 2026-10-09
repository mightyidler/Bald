const invoke=window.__TAURI__.core.invoke;

const i18n={korean:{applications:"애플리케이션",settings:"동작",automatic:"테두리 자동 제거",automaticDesc:"등록한 애플리케이션의 테두리를 자동으로 제거합니다",startup:"Windows 시작 시 Bald 실행",startupDesc:"Windows에 로그인하면 Bald를 실행합니다",environment:"개인 설정",language:"언어",theme:"테마",themeSystem:"시스템",themeLight:"라이트 모드",themeDark:"다크 모드",updates:"업데이트",currentVersion:"현재 버전",checkUpdate:"업데이트 확인",checking:"확인 중...",latest:"최신 버전입니다",installing:"업데이트 설치 중...",updateFailed:"확인 실패",addApplication:"애플리케이션 추가",search:"실행 중인 애플리케이션 검색",empty:"선택할 수 있는 애플리케이션이 없습니다.",add:"추가",added:"추가됨",reset:"설정 초기화",resetTitle:"설정 초기화",resetDesc:"모든 설정을 기본값으로 되돌리고 등록한 애플리케이션을 목록에서 제거합니다.",resetTargetLabel:"초기화 대상",resetTargetDesc:"테두리 자동 제거, 시작 프로그램, 언어, 테마와 등록된 애플리케이션 설정이 초기화됩니다.",resetSafetyLabel:"안전 장치",resetSafetyDesc:"현재 적용된 테두리는 복원되며 애플리케이션이나 개인 파일은 삭제되지 않습니다.",cancel:"취소",resetConfirm:"설정 초기화",exit:"Bald 종료"},english:{applications:"Applications",settings:"Behavior",automatic:"Automatic border removal",automaticDesc:"Automatically removes borders from registered applications",startup:"Launch Bald with Windows",startupDesc:"Starts Bald when you sign in to Windows",environment:"Preferences",language:"Language",theme:"Theme",themeSystem:"System",themeLight:"Light mode",themeDark:"Dark mode",updates:"Updates",currentVersion:"Current version",checkUpdate:"Check for updates",checking:"Checking...",latest:"Up to date",installing:"Installing update...",updateFailed:"Check failed",addApplication:"Add application",search:"Search running applications",empty:"No applications are available to select.",add:"Add",added:"Added",reset:"Reset settings",resetTitle:"Reset settings",resetDesc:"Restore every setting to its default and remove all registered applications from the list.",resetTargetLabel:"Will be reset",resetTargetDesc:"Automatic border removal, startup, language, theme, and registered application settings will be reset.",resetSafetyLabel:"Safety",resetSafetyDesc:"Applied borders will be restored. Applications and personal files will not be deleted.",cancel:"Cancel",resetConfirm:"Reset settings",exit:"Exit Bald"}};

let state=null,windows=[],language="korean",theme="system",elevationRequested=false;

const $=s=>document.querySelector(s);

let renderedApplicationsHtml="";

Object.assign(i18n.korean, {
  windowDrag: "창 이동",
  dragEnabled: "허용",
  dragDisabled: "차단",
  dragAllowedLabel: "창 이동 허용",
  dragBlockedLabel: "창 이동 차단"
});

Object.assign(i18n.english, {
  windowDrag: "Window dragging",
  dragEnabled: "Allowed",
  dragDisabled: "Blocked",
  dragAllowedLabel: "Window dragging allowed",
  dragBlockedLabel: "Window dragging blocked"
});

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
    await refresh();
  }
}

function esc(v){const e=document.createElement("span");e.textContent=v??"";

return e.innerHTML}

function applyTheme(){document.documentElement.dataset.theme=theme;$("#themeValue").textContent=i18n[language][theme==="light"?"themeLight":theme==="dark"?"themeDark":"themeSystem"];document.querySelectorAll("[data-theme]").forEach(e=>e.classList.toggle("is-selected",e.dataset.theme===theme))}

function applyLanguage(){const t=i18n[language];document.documentElement.lang=language==="korean"?"ko":"en";document.querySelectorAll("[data-i18n]").forEach(e=>e.textContent=t[e.dataset.i18n]);document.querySelectorAll("[data-i18n-placeholder]").forEach(e=>e.placeholder=t[e.dataset.i18nPlaceholder]);$("#languageValue").textContent=language==="korean"?"한국어":"English";document.querySelectorAll("[data-language]").forEach(e=>e.classList.toggle("is-selected",e.dataset.language===language));applyTheme()}

function applicationIcon(icon){return icon?`<img class="app-icon" src="${icon}" alt="">`:'<span class="app-icon-placeholder"></span>'}

function findLiveIcon(rule){const path=rule.executable_path?.toLowerCase();

return windows.find(w=>path&&w.executablePath?.toLowerCase()===path)?.icon||null}

function waitForMotion(ms){return matchMedia("(prefers-reduced-motion: reduce)").matches?Promise.resolve():new Promise(resolve=>setTimeout(resolve,ms))}

let toastTimer=null;

function showToast(message,duration=3000){const toast=$("#toast");toast.textContent=message;toast.classList.add("is-visible");clearTimeout(toastTimer);toastTimer=setTimeout(()=>toast.classList.remove("is-visible"),duration)}

function reportWindowError(error){showToast(`${language==="korean"?"창 적용·복구 실패":"Window apply/restore failed"}:\n${error}`,7000)}

async function checkForUpdates(){const button=$("#btnCheckUpdate"),t=i18n[language];

if(button.disabled)return;button.disabled=true;button.textContent=t.checking;

try{const result=await invoke("check_for_updates_manual");showToast(result==="LATEST"?t.latest:result,3000)}catch(error){showToast(`${t.updateFailed}:\n${error}`,3000)}finally{button.disabled=false;button.textContent=t.checkUpdate}}

const resetModal=$("#resetModal"),resetModalCard=resetModal.querySelector(".t-modal");

let resetCloseTimer=null,resetDimTimer=null;

function openResetModal(){clearTimeout(resetCloseTimer);clearTimeout(resetDimTimer);resetModalCard.classList.remove("is-closing");$("#dim").classList.add("is-open","is-reset");resetModal.classList.add("is-open");resetModalCard.classList.add("is-open");resetModal.setAttribute("aria-hidden","false");$("#resetCancel").focus({preventScroll:true})}

function closeResetModal(){const dim=$("#dim");dim.classList.remove("is-open");resetModal.classList.remove("is-open");resetModalCard.classList.remove("is-open");resetModalCard.classList.add("is-closing");resetModal.setAttribute("aria-hidden","true");const closeMs=parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--modal-close-dur"))||150;clearTimeout(resetCloseTimer);clearTimeout(resetDimTimer);resetCloseTimer=setTimeout(()=>resetModalCard.classList.remove("is-closing"),closeMs);resetDimTimer=setTimeout(()=>dim.classList.remove("is-reset"),260)}

async function removeHomeApplication(button){if(button.dataset.pending)return;button.dataset.pending="true";const row=button.closest(".app-row"),divider=row.previousElementSibling?.classList.contains("divider")?row.previousElementSibling:null;row.classList.add("is-removing");divider?.classList.add("is-removing");

try{await waitForMotion(300);await invoke("remove_application",{id:button.dataset.remove});await refresh()}catch(error){row.classList.remove("is-removing");divider?.classList.remove("is-removing");delete button.dataset.pending;reportWindowError(error);await refresh()}}

async function renderApplications() {
  const t = i18n[language], rules = state.config.applications;

  let html = "";

  for (const [index, rule] of rules.entries()) {
    if (index) html += '<div class="divider inset"></div>';

    const icon = state.applicationIcons?.[rule.id] || findLiveIcon(rule);

    html += `<div class="app-row">${applicationIcon(icon)}<div class="app-copy"><strong>${esc(rule.display_name)}</strong>${renderDragControl(rule, t)}</div><button class="remove-btn" data-remove="${rule.id}"><img src="figma/remove-x.svg" alt="삭제"></button></div>`;
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

try{if(wasAdded&&rule)await invoke("remove_application",{id:rule.id});else if(!wasAdded)await invoke("add_application",{choice});state=await invoke("get_state");const isAdded=Boolean(findAddedRule(choice));button.classList.toggle("is-added",isAdded);button.querySelector(".t-icon-swap").dataset.state=isAdded?"b":"a";await swapPickerText(button,i18n[language][isAdded?"added":"add"]);await renderApplications()}finally{delete button.dataset.pending}}

function renderPicker(){const t=i18n[language],query=$("#pickerSearch").value.trim().toLowerCase(),visible=windows.filter(w=>!query||w.title.toLowerCase().includes(query)||w.executableName.toLowerCase().includes(query));$("#pickerList").innerHTML=visible.map((w,index)=>{const added=Boolean(findAddedRule(w));

return `<div class="picker-row">${applicationIcon(w.icon)}<div class="app-copy"><strong>${esc(w.title)}</strong><span>${esc(w.executableName)}</span></div><button class="add-one-btn t-resize${added?" is-added":""}" data-index="${index}">${pickerActionIcon(added)}<span class="t-text-swap picker-action-label">${added?t.added:t.add}</span></button></div>`}).join("")||`<div class="picker-empty">${t.empty}</div>`;document.querySelectorAll(".add-one-btn").forEach(button=>button.onclick=()=>togglePickerApplication(button,visible[Number(button.dataset.index)]))}

async function openPicker(){windows=await invoke("list_windows");$("#pickerSearch").value="";renderPicker();$("#dim").classList.add("is-open");$("#picker").classList.add("is-open");$("#picker").setAttribute("aria-hidden","false");setTimeout(()=>$("#pickerSearch").focus(),360)}

function clearPickerDrag(){const picker=$("#picker");picker.classList.remove("is-dragging","is-drag-settling","is-drag-closing");picker.style.removeProperty("--picker-drag-y");picker.style.removeProperty("--picker-drag-scale");picker.style.transform=""}

function closePicker(){clearPickerDrag();$("#dim").classList.remove("is-open");$("#picker").classList.remove("is-open");$("#picker").setAttribute("aria-hidden","true")}

async function refresh(){state=await invoke("get_state");language=state.config.language;theme=state.config.theme||"system";applyLanguage();$("#automaticToggle").checked=state.config.automatic_application;$("#startupToggle").checked=state.startupEnabled;await renderApplications();

if(!elevationRequested&&Object.values(state.statuses||{}).includes("requires_elevation")){elevationRequested=true;invoke("elevate_border_service").catch(reportWindowError)}}

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
    const r = trigger.getBoundingClientRect(), appR = $(".app").getBoundingClientRect();
    const itemCount = menu.querySelectorAll(".dropdown-item").length;
    const menuHeight = itemCount * 42 + 8, spaceBelow = innerHeight - r.bottom - 8;
    const opensUp = spaceBelow < menuHeight && r.top > menuHeight;

    menu.classList.toggle("opens-up", opensUp);

    if (wrap.classList.contains("app-drag-control")) {
      menu.style.left = `${Math.max(8, Math.min(r.left, innerWidth - menu.offsetWidth - 8))}px`;
      menu.style.right = "auto";
    } else {
      menu.style.left = "auto";
      menu.style.right = `${innerWidth - appR.right + 20}px`;
    }

    menu.style.top = opensUp ? "auto" : `${r.bottom + 8}px`;
    menu.style.bottom = opensUp ? `${innerHeight - r.top + 8}px` : "auto";
  }

  wrap.classList.toggle("is-open", open);
  menu.classList.toggle("is-open", open);
  trigger.setAttribute("aria-expanded", String(open));
  $("#overlay").classList.toggle("is-open", open);
  openDD = open ? wrap.id : null;
}

const pickerDrag={active:false,pointerId:null,startY:0,lastY:0,lastTime:0,velocity:0,dy:0};

function beginPickerDrag(e){if(e.button!==0||e.target.closest("button")||!$("#picker").classList.contains("is-open"))return;pickerDrag.active=true;pickerDrag.pointerId=e.pointerId;pickerDrag.startY=pickerDrag.lastY=e.clientY;pickerDrag.lastTime=performance.now();pickerDrag.velocity=pickerDrag.dy=0;const picker=$("#picker");picker.classList.remove("is-drag-settling","is-drag-closing");picker.classList.add("is-dragging");e.currentTarget.setPointerCapture(e.pointerId)}

function movePickerDrag(e){if(!pickerDrag.active||e.pointerId!==pickerDrag.pointerId)return;e.preventDefault();const now=performance.now(),elapsed=Math.max(1,now-pickerDrag.lastTime),raw=e.clientY-pickerDrag.startY;pickerDrag.velocity=(e.clientY-pickerDrag.lastY)/elapsed;pickerDrag.lastY=e.clientY;pickerDrag.lastTime=now;pickerDrag.dy=raw;const upward=raw<0,dragY=upward?-Math.min(14,Math.pow(-raw,.65)):raw,scale=upward?1+Math.min(.008,-raw/15000):1;const picker=$("#picker");picker.style.setProperty("--picker-drag-y",`${dragY}px`);picker.style.setProperty("--picker-drag-scale",String(scale))}

function endPickerDrag(e){if(!pickerDrag.active||e.pointerId!==pickerDrag.pointerId)return;pickerDrag.active=false;const picker=$("#picker"),shouldClose=pickerDrag.dy>96||(pickerDrag.dy>36&&pickerDrag.velocity>.55),currentTransform=getComputedStyle(picker).transform;

try{e.currentTarget.releasePointerCapture(e.pointerId)}catch{}

if(matchMedia("(prefers-reduced-motion: reduce)").matches){if(shouldClose)closePicker();else clearPickerDrag();

return}

picker.style.transform=currentTransform;picker.classList.remove("is-dragging");picker.classList.add(shouldClose?"is-drag-closing":"is-drag-settling");picker.style.removeProperty("--picker-drag-y");picker.style.removeProperty("--picker-drag-scale");void picker.offsetHeight;picker.style.transform="";

if(shouldClose)$("#dim").classList.remove("is-open");let finished=false;

const finish=event=>{if(event&&event.target!==picker||event&&event.propertyName!=="transform"||finished)return;finished=true;clearTimeout(fallback);picker.removeEventListener("transitionend",finish);

if(shouldClose){picker.classList.remove("is-open");picker.setAttribute("aria-hidden","true")}

picker.classList.remove("is-drag-settling","is-drag-closing")};

picker.addEventListener("transitionend",finish);const fallback=setTimeout(()=>finish(),shouldClose?300:430)}

$("#pickerClose").onclick=closePicker;

$("#dim").onclick=closePicker;

$("#pickerSearch").oninput=renderPicker;

$("#automaticToggle").onchange=async e=>{try{await invoke("set_automatic",{enabled:e.target.checked})}catch(error){reportWindowError(error)}finally{await refresh()}};

$("#startupToggle").onchange=async e=>{await invoke("set_startup",{enabled:e.target.checked});await refresh()};

$("#btnCheckUpdate").onclick=checkForUpdates;

$("#resetBtn").onclick=openResetModal;

$("#resetCancel").onclick=closeResetModal;

$("#resetConfirm").onclick=async()=>{closeResetModal();await invoke("reset_settings");await refresh()};

$("#exitBtn").onclick=()=>invoke("exit_app");

const pickerHead=$(".picker-head");

pickerHead.addEventListener("pointerdown",beginPickerDrag);

pickerHead.addEventListener("pointermove",movePickerDrag);

pickerHead.addEventListener("pointerup",endPickerDrag);

pickerHead.addEventListener("pointercancel",endPickerDrag);

document.querySelectorAll(".dropdown-trigger").forEach(button=>button.onclick=e=>{e.stopPropagation();toggleDropdown(button.closest(".dropdown-wrap"))});

document.querySelectorAll("[data-language]").forEach(item=>item.onclick=async e=>{e.stopPropagation();await invoke("set_language",{language:item.dataset.language});closeMenus();await refresh()});

document.querySelectorAll("[data-theme]").forEach(item=>item.onclick=async e=>{e.stopPropagation();await invoke("set_theme",{theme:item.dataset.theme});closeMenus();await refresh()});

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
  prepareContentReveal();

  const generation = contentRevealGeneration;
  const stack = $(".content-stack");

  // Hidden sections must use their final font metrics before entering.
  await Promise.allSettled([
    document.fonts.load("400 17px Pretendard"),
    document.fonts.load("500 17px Pretendard"),
    document.fonts.load("600 17px Pretendard")
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

window.addEventListener("bald-window-hidden", prepareContentReveal);

window.addEventListener("bald-window-reopened", revealContent);

function updateHeaderScroll(){
  $(".header").classList.toggle("is-scrolled", $(".main").scrollTop > 0);
}

updateHeaderScroll();

$("#overlay").addEventListener("click",closeMenus);

$(".main").addEventListener("scroll",()=>{updateHeaderScroll();

if(openDD)closeMenus()},{passive:true});

document.addEventListener("keydown",e=>{if(e.key==="Escape"&&resetModal.classList.contains("is-open")){e.stopPropagation();closeResetModal();

return}

if(e.key==="Escape"){closeMenus();closePicker()}});

Promise.all([refresh(),invoke("get_app_version")]).then(([,version])=>$("#appVersionText").textContent=`v${version}`).catch(()=>$("#appVersionText").textContent="-").finally(revealContent);

setInterval(async()=>{if(!$("#picker").classList.contains("is-open"))await refresh()},2000);

$(".header").addEventListener("mousedown",e=>{if(e.button===0&&!e.target.closest("button"))invoke("start_window_drag")});

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
