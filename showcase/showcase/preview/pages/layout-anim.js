// layout-anim 页交互模拟（AI 手写，yio preview server 按页注入；不进打包）。
// Unity 侧等价接线在 ShowcaseRunner.WireLayoutAnimDrivers：#1/#3/#4 按钮 add_class
// 切换（CSS transition 起效），#6 是 C# TweenBuilder 运行时 API——浏览器侧用 rAF
// + cubic-out 复刻同一节奏（预览只求视觉可对照，非逐帧等价）。
import { ready } from '/yio-preview/lib/boot.js';

const easeOutCubic = (t) => 1 - Math.pow(1 - t, 3);

function tweenHeight(el, from, to, dur, done) {
  const t0 = performance.now();
  const step = (now) => {
    const t = Math.min(1, (now - t0) / dur);
    el.style.height = (from + (to - from) * easeOutCubic(t)) + 'px';
    if (t < 1) requestAnimationFrame(step);
    else if (done) done();
  };
  requestAnimationFrame(step);
}

ready.then(() => {
  // #1 折叠面板：height 0↔200px（transition，与 Unity 同走 CSS）。
  const fold = document.getElementById('fold-body');
  const btnFold = document.getElementById('btn-fold');
  if (fold && btnFold) {
    btnFold.addEventListener('click', () => {
      const open = fold.classList.toggle('open');
      btnFold.textContent = open ? '收起' : '展开';
    });
  }
  // #3 侧栏收起：flex-grow 换手。
  const pair = document.getElementById('sidebar-pair');
  const btnSide = document.getElementById('btn-sidebar');
  if (pair && btnSide) {
    btnSide.addEventListener('click', () => {
      const collapsed = pair.classList.toggle('collapsed');
      btnSide.textContent = collapsed ? '展开侧栏' : '收起侧栏';
    });
  }
  // #4 响应式面板：width 50vw↔80vw。
  const vw = document.getElementById('vw-panel');
  const btnVw = document.getElementById('btn-vw');
  if (vw && btnVw) {
    btnVw.addEventListener('click', () => {
      const wide = vw.classList.toggle('wide');
      btnVw.textContent = wide ? '缩回' : '拉宽';
    });
  }
  // #6 TweenBuilder.Height 摆台：60↔220px，0.6s cubic-out（Unity 侧是运行时 API）。
  const panel = document.getElementById('tween-panel');
  const btnTween = document.getElementById('btn-tween');
  if (panel && btnTween) {
    let tall = false;
    btnTween.addEventListener('click', () => {
      tall = !tall;
      tweenHeight(panel, tall ? 60 : 220, tall ? 220 : 60, 600);
      btnTween.textContent = tall ? 'C# 回落' : 'C# 动画';
    });
  }
});
