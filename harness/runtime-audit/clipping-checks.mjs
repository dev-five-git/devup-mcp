// Animation never exempts an ancestor from checking unrelated clipped content.
export function observeClippedContent(page, allowedMarquees = []) {
  return page.evaluate((selectors) => {
    const clipped = [], exemptions = new Set(), invalidExemptions = [];
    for (const selector of selectors) {
      let found;
      try { found = document.querySelectorAll(selector); } catch { found = []; }
      const box = found.length === 1 ? found[0] : null;
      const track = box?.children.length === 1 ? box.firstElementChild : null;
      const style = box && getComputedStyle(box), trackStyle = track && getComputedStyle(track);
      const loopingTranslation = track?.getAnimations().some((animation) => {
        const timing = animation.effect?.getTiming();
        if(timing?.iterations!==Infinity||typeof timing.duration!=='number'||timing.duration<=0||!animation.effect?.getKeyframes().some(frame=>frame.transform||frame.translate))return false;
        const time=animation.currentTime,state=animation.playState;
        try {
          animation.pause();animation.currentTime=0;const first=track.getBoundingClientRect();
          animation.currentTime=timing.duration*.5;const middle=track.getBoundingClientRect();
          return Math.abs(first.x-middle.x)>.1&&Math.abs(first.y-middle.y)<.5&&Math.abs(first.width-middle.width)<.5&&Math.abs(first.height-middle.height)<.5;
        }finally{animation.currentTime=time;if(state==='running')animation.play();}
      });
      const safe = box && box !== document.body && box !== document.documentElement
        && box.checkVisibility({opacityProperty:true, visibilityProperty:true})
        && ['hidden','clip'].includes(style.overflowX)
        && box.scrollWidth > box.clientWidth + 1 && box.scrollHeight <= box.clientHeight + 1
        && track && trackStyle.whiteSpace === 'nowrap' && loopingTranslation
        && !box.querySelector('a,button,input,select,textarea,[tabindex]');
      if (safe) exemptions.add(box);
      else invalidExemptions.push({selector, reason:'Expected one visible horizontal-only clipped marquee with one nowrap, infinitely translating track and no controls.'});
    }
    const probe = document.createElement('div');
    probe.style.cssText = 'position:absolute;top:-999px;width:100px;height:100px;overflow:scroll';
    (document.body ?? document.documentElement).append(probe);
    const browserHidesScrollbars = probe.offsetWidth - probe.clientWidth === 0;
    probe.remove();
    for (const element of document.querySelectorAll('body *')) {
      if (!element.checkVisibility({opacityProperty:true, visibilityProperty:true})) continue;
      const style = getComputedStyle(element), box = element.getBoundingClientRect();
      if (!box.width || !box.height) continue;
      const horizontal = style.overflowX !== 'visible' && element.scrollWidth > element.clientWidth + 1;
      const vertical = ['hidden','clip'].includes(style.overflowY) && element.scrollHeight > element.clientHeight + 1;
      if (!horizontal && !vertical) continue;
      if (style.textOverflow === 'ellipsis' || (style.webkitLineClamp && style.webkitLineClamp !== 'none')) continue;
      const gutter = element.offsetHeight - element.clientHeight - parseFloat(style.borderTopWidth) - parseFloat(style.borderBottomWidth) > 2;
      const scrollbar = gutter || (browserHidesScrollbars && style.scrollbarWidth !== 'none');
      if (!vertical && ['auto','scroll'].includes(style.overflowX) && scrollbar) continue;
      if (element.closest('[aria-hidden="true"]')) continue;
      const media = [...element.querySelectorAll('img,svg,video,canvas')];
      const decorative = style.pointerEvents === 'none' && !element.textContent.trim()
        && media.every((node) => node.tagName === 'IMG' ? node.getAttribute('alt') === '' : node.tagName.toLowerCase() === 'svg');
      if (decorative || (!vertical && exemptions.has(element))) continue;
      const controls = element.parentElement?.querySelector('button[aria-label],[role="scrollbar"]');
      if (horizontal && !vertical && element.dataset.auditCarousel === 'true' && (scrollbar || controls)) continue;
      clipped.push({name:element.id || `${element.tagName.toLowerCase()} "${element.textContent.trim().slice(0,60)}"`, horizontal, vertical});
    }
    return selectors.length ? {clipped, invalidExemptions} : {clipped};
  }, allowedMarquees);
}
