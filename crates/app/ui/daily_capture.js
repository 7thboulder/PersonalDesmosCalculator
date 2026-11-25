// Runs in the hidden panel that loads dailyintegral.com's own pages (never
// in the app's panel). The pages fetch each problem and its hints from the
// site's server; this passes copies of those answers to the app, which
// keeps them in the daily problems archive. Nothing on the page changes.
(function () {
  if (window.__dailyCapture) return;
  window.__dailyCapture = true;
  var send = function (m) { try { window.ipc.postMessage(JSON.stringify(m)); } catch (e) {} };
  var original = window.fetch;
  if (!original) return;
  window.fetch = function (input, init) {
    var url = typeof input === 'string' ? input : (input && input.url) || String(input);
    var promise = original.apply(this, arguments);
    var m = /\/functions\/v1\/(get-daily-puzzle|get-puzzle-hint|get-server-day)\b/.exec(url);
    if (m) {
      var body = init && typeof init.body === 'string' ? init.body : null;
      promise.then(function (r) { return r.clone().text(); }).then(function (text) {
        send({ type: 'dailyCaptured', fn: m[1], request: body, response: text, page: location.pathname });
      }).catch(function () {});
    }
    return promise;
  };
})();
