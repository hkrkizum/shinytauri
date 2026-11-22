(() => {
  const steps = {
    prepare: ["起動環境を準備中", "ホストを起動"],
    r: ["R を準備中", "R プロセスを起動"],
    load: ["Shiny アプリの応答を待機中", "Shiny アプリの起動を開始"],
    http: ["Shiny アプリとの接続を確認中", "接続先を受け取り、HTTP 応答の確認を開始"],
    open: ["Shiny アプリを表示中", "Shiny アプリの HTTP 応答を確認"],
    failed: ["アプリを起動できませんでした", "起動に失敗"],
    stopped: ["Shiny サーバーが停止しました", "Shiny サーバーが停止"],
  };

  const byId = (id) => document.getElementById(id);
  const main = document.querySelector("main");
  const history = byId("startup-events");
  const files = byId("log-files");

  let snapshotTime = -1;
  let elapsedAtReceipt = 0;
  let receivedAt = 0;
  let timer;
  function elapsed() {
    return elapsedAtReceipt + (timer === undefined ? 0 : performance.now() - receivedAt);
  }
  function renderElapsed() {
    const seconds = Math.floor(elapsed() / 1000);
    const text = [Math.floor(seconds / 60), seconds % 60]
      .map((part) => String(part).padStart(2, "0")).join(":");
    if (byId("startup-elapsed").textContent !== text) byId("startup-elapsed").textContent = text;
  }
  function showFile(containerId, pathId, path) {
    byId(containerId).hidden = !path;
    byId(pathId).textContent = path || "";
  }
  function appendEvent({ event, elapsed_ms }) {
    const row = document.createElement("li");
    row.dataset.event = event;
    const timestamp = document.createElement("time");
    const seconds = (elapsed_ms / 1000).toFixed(3);
    timestamp.dateTime = `PT${seconds}S`;
    timestamp.textContent = `+${seconds} 秒`;
    const message = document.createElement("span");
    message.textContent = Object.hasOwn(steps, event) ? steps[event][1] : event;
    row.append(timestamp, message);
    history.append(row);
  }

  window.shinytauriProgress = (snapshot) => {
    const events = snapshot.events;
    if (!events.length || events.length < history.children.length ||
        (events.length === history.children.length && snapshot.elapsed_ms < snapshotTime)) return;

    const latest = events[events.length - 1];
    const stage = latest.event;
    const terminal = stage === "failed" || stage === "stopped";
    const previousStage = main.dataset.stage;

    // A replay can be older than the elapsed time already shown between host updates.
    elapsedAtReceipt = terminal ? latest.elapsed_ms : Math.max(snapshot.elapsed_ms, elapsed());
    receivedAt = performance.now();
    snapshotTime = snapshot.elapsed_ms;
    if (terminal) {
      clearInterval(timer);
      timer = undefined;
    } else if (timer === undefined) {
      timer = setInterval(renderElapsed, 250);
    }
    renderElapsed();

    if (snapshot.title) {
      document.title = snapshot.title;
      byId("startup-title").textContent = snapshot.title;
    }
    main.dataset.stage = stage;
    if (stage !== previousStage) {
      byId("startup-stage").textContent = Object.hasOwn(steps, stage) ? steps[stage][0] : stage;
    }
    byId("startup-error").textContent = snapshot.error || "";
    byId("startup-error").hidden = !snapshot.error;

    for (const event of events.slice(history.children.length)) appendEvent(event);
    byId("history").hidden = false;

    showFile("host-file", "host-log", snapshot.host_log);
    showFile("r-file", "r-log", snapshot.r_log);
    files.hidden = !snapshot.host_log && !snapshot.r_log;
    if (terminal && stage !== previousStage) files.open = true;
  };
})();
