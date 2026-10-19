//! User-triggered on-device page translation through the Microsoft Edge Translator API.

pub const PAGE_TRANSLATION_SCRIPT: &str = r#"(() => {
  const overlayId = '__pip_translate_status';
  const show = text => {
    let node = document.getElementById(overlayId);
    if (!node) {
      node = document.createElement('div');
      node.id = overlayId;
      node.setAttribute('role', 'status');
      node.setAttribute('aria-live', 'polite');
      node.style.cssText = 'all:initial;position:fixed;z-index:2147483647;top:12px;right:12px;max-width:360px;padding:12px 16px;border:1px solid #efc49f;border-radius:10px;background:#fffaf5;color:#17202e;font:14px/1.45 Segoe UI,sans-serif;box-shadow:0 4px 18px #0003';
      document.documentElement.appendChild(node);
    }
    node.textContent = text;
  };
  const hide = () => document.getElementById(overlayId)?.remove();
  const restore = state => {
    try {
      for (const item of state.originals) {
        if (item.node.isConnected && item.node.textContent === item.translated) item.node.textContent = item.original;
      }
    } finally {
      try { state.session?.destroy?.(); } catch (_) {}
      if (window.__pipTranslation === state) delete window.__pipTranslation;
      hide();
    }
  };

  const current = window.__pipTranslation;
  if (current?.status === 'working') return;
  if (current?.status === 'done') {
    restore(current);
    return;
  }

  void (async () => {
    const detector = globalThis.LanguageDetector;
    const TranslatorApi = globalThis.Translator;
    if (!detector || !TranslatorApi) {
      show('WebView2 này chưa có dịch trang trên thiết bị. Pip không gửi nội dung sang dịch vụ dịch bên ngoài.');
      setTimeout(hide, 6500);
      return;
    }

    const skipped = 'SCRIPT STYLE NOSCRIPT TEXTAREA INPUT SELECT OPTION CODE PRE SVG IFRAME CANVAS'.split(' ');
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode(node) {
        const text = node.nodeValue || '';
        const parent = node.parentElement;
        if (!parent || !text.trim() || text.trim().length > 600 || parent.isContentEditable) return NodeFilter.FILTER_REJECT;
        if (parent.closest('[hidden],[aria-hidden="true"],[data-pip-translate-ui]')) return NodeFilter.FILTER_REJECT;
        for (let element = parent; element && element !== document.body; element = element.parentElement) {
          if (skipped.includes(element.tagName)) return NodeFilter.FILTER_REJECT;
        }
        return NodeFilter.FILTER_ACCEPT;
      }
    });
    const nodes = [];
    let totalChars = 0;
    while (walker.nextNode() && nodes.length < 600 && totalChars < 30000) {
      const node = walker.currentNode;
      const original = node.nodeValue || '';
      nodes.push({node, original});
      totalChars += original.length;
    }
    if (!nodes.length) {
      show('Trang này không có nội dung văn bản để dịch.');
      setTimeout(hide, 6500);
      return;
    }

    const state = {status:'working', originals:[], session:null};
    window.__pipTranslation = state;
    show('Đang nhận diện ngôn ngữ của trang…');
    try {
      const sample = nodes.slice(0, 80).map(item => item.original).join(' ').slice(0, 4000);
      const detectorSession = await detector.create();
      let detected;
      try {
        detected = await detectorSession.detect(sample);
      } finally {
        detectorSession.destroy?.();
      }
      const source = detected?.[0]?.detectedLanguage?.split('-')[0];
      if (!source) throw new Error('Không nhận diện được ngôn ngữ nguồn.');
      if (source === 'vi') {
        delete window.__pipTranslation;
        show('Trang đã là tiếng Việt.');
        setTimeout(hide, 6500);
        return;
      }
      const availability = await TranslatorApi.availability({sourceLanguage:source,targetLanguage:'vi'});
      if (availability === 'unavailable') throw new Error('Chưa có mô hình dịch cho cặp ngôn ngữ này.');

      const session = await TranslatorApi.create({
        sourceLanguage:source,
        targetLanguage:'vi',
        monitor(monitor) {
          monitor.addEventListener('downloadprogress', event => {
            if (window.__pipTranslation === state) {
              const percent = event.total ? Math.floor(event.loaded / event.total * 100) : 0;
              show(`Đang tải mô hình dịch trên thiết bị… ${percent}%`);
            }
          });
        }
      });
      state.session = session;
      let cursor = 0;
      let translatedCount = 0;
      let failure = null;
      const worker = async () => {
        try {
          while (!failure && cursor < nodes.length) {
            const item = nodes[cursor++];
            const match = item.original.match(/^(\s*)([\s\S]*?)(\s*)$/);
            const translatedText = await session.translate(match[2]);
            if (failure) return;
            const translated = match[1] + translatedText + match[3];
            if (!item.node.isConnected || item.node.nodeValue !== item.original) continue;
            item.node.nodeValue = translated;
            item.translated = translated;
            state.originals.push(item);
            translatedCount++;
            if (translatedCount % 8 === 0 || translatedCount === nodes.length) {
              show(`Đang dịch trang sang tiếng Việt… ${translatedCount}/${nodes.length}`);
            }
          }
        } catch (error) {
          failure ??= error;
        }
      };
      await Promise.all(Array.from({length:Math.min(4,nodes.length)}, worker));
      if (failure) throw failure;
      state.status = 'done';
      show(`Đã dịch ${translatedCount} đoạn trên thiết bị. Chọn “Dịch trang” lần nữa để khôi phục.`);
    } catch (error) {
      restore(state);
      show(`Pip chưa dịch được trang: ${String(error?.message || error).slice(0,180)}`);
      setTimeout(hide, 6500);
    }
  })();
})()"#;

#[cfg(test)]
mod tests {
    use super::PAGE_TRANSLATION_SCRIPT;

    #[test]
    fn translator_script_is_explicit_and_reversible() {
        assert!(PAGE_TRANSLATION_SCRIPT.contains("globalThis.LanguageDetector"));
        assert!(PAGE_TRANSLATION_SCRIPT.contains("targetLanguage:'vi'"));
        assert!(PAGE_TRANSLATION_SCRIPT.contains("item.node.textContent === item.translated"));
        assert!(PAGE_TRANSLATION_SCRIPT.contains("không gửi nội dung sang dịch vụ dịch bên ngoài"));
        assert!(PAGE_TRANSLATION_SCRIPT.contains("if (failure) return"));
        assert!(PAGE_TRANSLATION_SCRIPT.contains("detectorSession.destroy?.()"));
    }
}
