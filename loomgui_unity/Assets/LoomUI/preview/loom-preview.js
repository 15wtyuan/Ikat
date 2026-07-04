// LoomGUI showcase 浏览器预览 driver（preview-only——打包器只遍历 body，head 不进 pkg.bin）。
// 移植 LoomShowcaseDriver 的"够看视觉"子集：导航 + 动态页内容 + overlay。
// 非忠实行为镜像——虚拟列表不做 slot 复用、tween 不移植 ease 数学（视觉验证导向）。
// 经典脚本（非 ES module）——后者在 file:// 被 CORS 拦。
(function () {
  'use strict';

  // === CONFIG ===
  // home nav 按钮 id → 目标页文件名（抄 LoomShowcaseDriver.SubscribeHome）。
  var NAV = {
    'nav-controls': 'page_controls',
    'nav-text': 'page_text',
    'nav-image': 'page_image',
    'nav-scroll': 'page_scroll',
    'nav-tween': 'page_tween',
    'nav-interact': 'page_interact',
    'nav-dyntree': 'page_dyntree',
    'nav-list': 'page_list'
  };

  // overlay 组件模板：<style> + markup 一起（宿主页没这些类的 CSS）。
  // mail 的 .root 改名 .loom-mail-root 避撞宿主页 .root(1080×1920)。
  // 改了 mail.html/tips_toast.html 须同步这里。
  var TEMPLATES = {
    mail:
      '<style>' +
      '.loom-mail-root{width:600px;height:800px;background-color:#1a1d2e;flex-direction:column;padding:16px;gap:12px}' +
      '.loom-mail-root .header{flex-direction:row;justify-content:space-between;align-items:center;padding-bottom:8px;border-width:0 0 1px 0;border-color:#3a3f55}' +
      '.loom-mail-root .title{color:#e6e6e0;font-size:28px;font-weight:700}' +
      '.loom-mail-root .count{color:#5fb2c4;font-size:16px}' +
      '.loom-mail-root .list{flex-direction:column;gap:8px;flex:1}' +
      '.loom-mail-root .mail-item{flex-direction:row;gap:12px;padding:12px;background-color:#2a2f45;border-radius:8px;align-items:center}' +
      '.loom-mail-root .mail-icon{width:40px;height:40px;border-radius:20px;color:#ffffff;font-size:18px;font-weight:700;justify-content:center;align-items:center}' +
      '.loom-mail-root .mail-body{flex-direction:column;gap:4px;flex:1}' +
      '.loom-mail-root .mail-from{color:#e6e6e0;font-size:16px;font-weight:600}' +
      '.loom-mail-root .mail-sub{color:#8a8fa3;font-size:14px}' +
      '.loom-mail-root .footer{flex-direction:row;justify-content:center;padding-top:8px}' +
      '.loom-mail-root .btn{background-color:#5fb2c4;color:#ffffff;font-size:18px;font-weight:600;padding:10px 32px;border-radius:6px}' +
      '</style>' +
      '<div class="loom-mail-root">' +
        '<div class="header"><span class="title">邮件</span><span class="count">3 封未读</span></div>' +
        '<div class="list">' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#5fb2c4">系</div><div class="mail-body"><span class="mail-from">系统奖励</span><span class="mail-sub">每日登录奖励已发放</span></div></div>' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#c2605a">战</div><div class="mail-body"><span class="mail-from">竞技场</span><span class="mail-sub">赛季结束，你的排名 127</span></div></div>' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#6fa66c">友</div><div class="mail-body"><span class="mail-from">好友小明</span><span class="mail-sub">送了你 100 金币</span></div></div>' +
        '</div>' +
        '<div class="footer"><button class="btn">一键领取</button></div>' +
      '</div>',
    tips:
      '<style>' +
      '.loom-toast{background-color:#252839;border:1px solid #5fb2c4;border-radius:8px;padding:20px 32px;gap:10px;align-items:center;box-shadow:0 4px 12px #0008}' +
      '.loom-toast .toast-icon{font-size:28px;color:#5fb2c4}' +
      '.loom-toast .toast-text{color:#e0e0e0;font-size:20px}' +
      '.loom-toast .toast-sub{color:#9aa0b4;font-size:14px}' +
      '</style>' +
      '<div class="loom-toast">' +
        '<div class="toast-icon">✦</div>' +
        '<div style="flex-direction:column;gap:4px">' +
          '<div class="toast-text">操作成功</div>' +
          '<div class="toast-sub">tips_layer 叠加演示（定时摘除）</div>' +
        '</div>' +
      '</div>'
  };

  // === shared helpers ===
  function $(id) { return document.getElementById(id); }
  function bind(id, type, fn) { var el = $(id); if (el) el.addEventListener(type, fn); }
  function bindClick(id, fn) { bind(id, 'click', fn); }

  // 导航：所有页 #back-home → home；home 的 #nav-* → 各页。
  // location.href 按【文档 URL】解析（不受 <base> 影响，base 只管 <img>/<link>/<script>）。
  // 各页同在 showcase/，故用同目录相对名 'xxx.html'——带 'showcase/' 前缀会变 .../showcase/showcase/ 404。
  function wireBackHome() {
    bindClick('back-home', function () { location.href = 'home.html'; });
  }
  function wireNav() {
    Object.keys(NAV).forEach(function (navId) {
      bindClick(navId, function () { location.href = NAV[navId] + '.html'; });
    });
  }

  // overlay 容器（position:fixed 盖页面）。tips 底部居中、pointer-events:none；
  // mail 居中 + 半透明遮罩。
  function ensureOverlay(kind) {
    var id = 'loom-overlay-' + kind;
    var existing = document.getElementById(id);
    if (existing) return existing;
    var ov = document.createElement('div');
    ov.id = id;
    if (kind === 'tips') {
      ov.style.cssText = 'position:fixed;inset:0;flex-direction:column;align-items:center;justify-content:flex-end;padding:40px;pointer-events:none;z-index:50';
    } else {
      ov.style.cssText = 'position:fixed;inset:0;flex-direction:column;align-items:center;justify-content:center;background:rgba(0,0,0,.5);z-index:60';
    }
    document.body.appendChild(ov);
    return ov;
  }
  var tipsTimer = null;
  function showTips() {
    var ov = ensureOverlay('tips');
    ov.innerHTML = TEMPLATES.tips;
    if (tipsTimer) clearTimeout(tipsTimer);
    tipsTimer = setTimeout(function () { ov.innerHTML = ''; tipsTimer = null; }, 2000);
  }
  function showMail() { ensureOverlay('mail').innerHTML = TEMPLATES.mail; }
  function hideMail() { var ov = $('loom-overlay-mail'); if (ov) ov.innerHTML = ''; }

  // 灯阵脉冲：lamp-{name} 容器 opacity 1→0.3→1（CSS transition，近似 C# LightLamp）。
  function pulseLamp(name) {
    var c = $('lamp-' + name);
    if (!c) return;
    c.style.transition = 'opacity .2s';
    c.style.opacity = '0.3';
    setTimeout(function () { c.style.opacity = '1'; }, 200);
  }

  // fit-scale：镜像 engine letterbox（sf=min(vw/1080, vh/1920)）。
  // Chrome/Edge 支持 body.zoom；旧 Firefox 无效（降级不缩放，用户 Ctrl +-）。
  function applyFitScale() {
    var sf = Math.min(window.innerWidth / 1080, window.innerHeight / 1920);
    // sf>=1 时清空 zoom——否则小窗设了 zoom 后放大窗口，stale 缩放留痕。
    document.body.style.zoom = (sf > 0 && sf < 1) ? sf : '';
  }

  // === page handlers（Task 5-10 填充，这里 stub 只调 wireBackHome） ===
  var pages = {
    home:          function () {
      wireBackHome();
      wireNav();
      bindClick('nav-tips-demo', showTips);
    },
    page_controls: function () {
      wireBackHome();
      var slot = $('model-slot');
      if (slot) {
        slot.style.justifyContent = 'center';
        slot.style.alignItems = 'center';
        slot.innerHTML = '<div style="color:#9aa0b4;font-size:12px;text-align:center;padding:8px">[NativeHost<br>外部 GO<br>预览不支持]</div>';
      }
    },
    page_text:     function () { wireBackHome(); },
    page_image:    function () { wireBackHome(); },
    page_scroll:   function () { wireBackHome(); },
    page_tween:    function () {
      wireBackHome();
      // 注入 tween 动画样式：.play = 末态（CSS transition 驱动，不移植 ease 数学）。
      var st = document.createElement('style');
      st.textContent =
        '#t-opacity,#t-translate,#t-scale,#t-rotate,#t-bgcolor,#t-textcolor,' +
        '#ease-0,#ease-1,#ease-2,#d-0,#d-1,#d-2{transition:all .8s ease}' +
        '#t-opacity.play{opacity:0}' +
        '#t-translate.play{transform:translateX(40px)}' +
        '#t-scale.play{transform:scale(1.4)}' +
        '#t-rotate.play{transform:rotate(360deg)}' +
        '#t-bgcolor.play{background-color:#6fa66c}' +
        '#t-textcolor.play{color:#c2605a}' +
        '#ease-0.play,#ease-1.play,#ease-2.play{transform:translateX(200px)}' +
        '#d-0.play,#d-1.play,#d-2.play{opacity:1}' +
        '#kill-target{animation:loom-spin 4s linear infinite}' +
        '@keyframes loom-spin{to{transform:rotate(360deg)}}' +
        '#kill-target.paused{animation-play-state:paused}' +
        '#kill-target.cleared{animation:none;transform:none}';
      document.head.appendChild(st);
      // d-0/1/2 初始隐（delay 错峰从隐到显）。
      ['d-0', 'd-1', 'd-2'].forEach(function (id) { var el = $(id); if (el) el.style.opacity = '0'; });
      function tg(id) { var el = $(id); if (el) el.classList.toggle('play'); }
      // 6 prop 同放（末态来自上面 .play 规则）。
      bindClick('tween-play', function () {
        ['t-opacity', 't-translate', 't-scale', 't-rotate', 't-bgcolor', 't-textcolor'].forEach(tg);
      });
      // 3 条近似 ease（QuadIn / CubicOut / BackInOut）。
      bindClick('ease-play', function () {
        var eases = ['cubic-bezier(.55,.085,.68,.53)', 'cubic-bezier(.215,.61,.355,1)', 'cubic-bezier(.68,-.55,.265,1.55)'];
        ['ease-0', 'ease-1', 'ease-2'].forEach(function (id, i) {
          var el = $(id);
          if (el) { el.style.transition = 'transform 1s ' + eases[i]; el.classList.toggle('play'); }
        });
      });
      // delay 错峰：递增 transition-delay。
      bindClick('delay-play', function () {
        ['d-0', 'd-1', 'd-2'].forEach(function (id, i) {
          var el = $(id);
          if (el) { el.style.transitionDelay = (i * 0.2) + 's'; el.classList.toggle('play'); }
        });
      });
      // complete：t-opacity 动画结束后亮灯。
      bindClick('complete-play', function () {
        var el = $('t-opacity');
        if (!el) return;
        el.classList.toggle('play');
        el.addEventListener('transitionend', function done() {
          el.removeEventListener('transitionend', done);
          pulseLamp('complete');
        });
      });
      // kill 冻结当前角（pause）；clear 清动画回 CSS 初始。
      bindClick('kill-btn', function () {
        var el = $('kill-target');
        if (el) { el.classList.add('paused'); el.classList.remove('cleared'); }
      });
      bindClick('clear-btn', function () {
        var el = $('kill-target');
        if (el) { el.classList.remove('paused'); el.classList.add('cleared'); }
      });
    },
    page_interact: function () {
      wireBackHome();
      // click / dblclick
      bind('hit-click', 'click', function () { pulseLamp('click'); });
      bind('hit-click', 'dblclick', function () { pulseLamp('click'); });
      // hover（RollOver/Out）
      bind('hit-hover', 'mouseenter', function () { pulseLamp('hover'); });
      bind('hit-hover', 'mouseleave', function () { pulseLamp('hover'); });
      // drag（HTML5 dragstart/drag）
      bind('hit-drag', 'dragstart', function () { pulseLamp('drag'); });
      bind('hit-drag', 'drag', function () { pulseLamp('drag'); });
      // longpress（mousedown 起 1.5s timer）
      var lpTimer = null;
      var lp = $('hit-longpress');
      if (lp) {
        lp.addEventListener('mousedown', function () {
          lpTimer = setTimeout(function () { pulseLamp('longpress'); }, 1500);
        });
        lp.addEventListener('mouseup', function () { if (lpTimer) clearTimeout(lpTimer); });
        lp.addEventListener('mouseleave', function () { if (lpTimer) clearTimeout(lpTimer); });
      }
      // key（聚焦后按键）
      bind('hit-key', 'keydown', function () { pulseLamp('key'); });
      // 路由：inner stopPropagation 止冒泡。
      bind('route-outer', 'click', function () { pulseLamp('route'); });
      bind('route-pe', 'click', function () { pulseLamp('route'); });
      bind('route-inner', 'click', function (e) { e.stopPropagation(); pulseLamp('route'); });
      // hit-disabled 不绑（HTML 已带 .disabled，视觉灰 + 不响应）
    },
    page_dyntree:  function () {
      wireBackHome();
      var dynPanels = [];
      var dynSeq = 0;
      var dynStyleToggled = false;
      var anchor = $('dyn-anchor');
      // panel 视觉对齐 C# CreateDynPanel（深色卡 + span 标题 + img icon）。
      function createPanel() {
        dynSeq++;
        var panel = document.createElement('div');
        panel.style.cssText = 'width:120px;height:90px;background:#2a2f45;border-radius:8px;flex-direction:column;gap:4px;padding:6px';
        var title = document.createElement('span');
        title.style.cssText = 'font-size:14px;color:#e6e6e0';
        title.textContent = 'item-' + dynSeq;
        panel.appendChild(title);
        var icon = document.createElement('img');
        icon.src = 'res/icons/skin.png';
        icon.style.cssText = 'width:40px;height:40px';
        panel.appendChild(icon);
        return panel;
      }
      bindClick('dyn-add', function () { if (anchor) { var p = createPanel(); anchor.appendChild(p); dynPanels.push(p); } });
      bindClick('dyn-add20', function () {
        if (!anchor) return;
        var frag = document.createDocumentFragment();
        for (var i = 0; i < 20; i++) { var p = createPanel(); frag.appendChild(p); dynPanels.push(p); }
        anchor.appendChild(frag);
      });
      bindClick('dyn-del', function () {
        var last = dynPanels.pop();
        if (last && last.parentNode) last.parentNode.removeChild(last);
      });
      bindClick('dyn-clear', function () {
        dynPanels.forEach(function (p) { if (p.parentNode) p.parentNode.removeChild(p); });
        dynPanels = [];
      });
      bindClick('dyn-style', function () {
        if (!dynPanels.length) return;
        var last = dynPanels[dynPanels.length - 1];
        dynStyleToggled = !dynStyleToggled;
        last.style.cssText = dynStyleToggled
          ? 'background:#c2605a;width:160px;height:70px;border-radius:16px;flex-direction:column;gap:4px;padding:6px'
          : 'width:120px;height:90px;background:#2a2f45;border-radius:8px;flex-direction:column;gap:4px;padding:6px';
      });
      bindClick('dyn-load-mail', function () {
        showMail();
        var s = $('dyn-load-status'); if (s) s.textContent = '当前：mail';
      });
      bindClick('dyn-load-showcase', function () {
        hideMail();
        var s = $('dyn-load-status'); if (s) s.textContent = '当前：showcase';
      });
    },
    page_list:     function () {
      wireBackHome();
      // item 视觉对齐 C# VirtualListDriver.CreateItem（灰底 + icon + 标题）。
      // icon 用 res/icons/skin.png（<base href=".."> 解析到 LoomUI/res/）。
      function renderItem(height, title) {
        var row = document.createElement('div');
        row.style.cssText = 'width:100%;height:' + height + 'px;flex-direction:row;align-items:center;gap:12px;padding:0 16px;background-color:#252839';
        var icon = document.createElement('img');
        icon.src = 'res/icons/skin.png';
        icon.style.cssText = 'width:48px;height:48px';
        row.appendChild(icon);
        var t = document.createElement('span');
        t.style.cssText = 'color:#e0e0e0;font-size:20px';
        t.textContent = title;
        row.appendChild(t);
        return row;
      }
      // equal：1000 个等高 80px。
      var eq = $('list-equal');
      if (eq) {
        var f1 = document.createDocumentFragment();
        for (var i = 0; i < 1000; i++) f1.appendChild(renderItem(80, 'Item ' + i));
        eq.appendChild(f1);
      }
      // variable：200 个 sin 高（60~140px，抄 C# sizes[i]=100+40*sin(i*0.3)）。
      var vr = $('list-variable');
      if (vr) {
        var f2 = document.createDocumentFragment();
        for (var j = 0; j < 200; j++) {
          var h = 100 + 40 * Math.sin(j * 0.3);
          f2.appendChild(renderItem(h, 'Item ' + j + '  (' + Math.round(h) + 'px)'));
        }
        vr.appendChild(f2);
      }
    }
  };

  // === boot ===
  function boot() {
    applyFitScale();
    window.addEventListener('resize', applyFitScale);
    var file = (location.pathname.split('/').pop() || 'home.html').replace('.html', '');
    if (pages[file]) pages[file]();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
