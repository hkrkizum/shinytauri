start_browser <- function() {
  previous <- options(chromote.timeout = 30)
  on.exit(options(previous), add = TRUE)
  # AppDriver skips when Chromium cannot start. Browser setup errors must fail CI.
  tryCatch(
    shinytest2::AppDriver$new(),
    skip = function(e) stop(conditionMessage(e), call. = FALSE)
  )
}

testthat::test_that("navigation and browser history preserve the counter", {
  app <- start_browser()
  on.exit(app$stop(), add = TRUE)

  app$wait_for_js(
    "document.querySelector('nav a[href$=\"#!/report\"]') !== null"
  )
  testthat::expect_equal(
    app$get_value(output = "app-counter-count"),
    "カウント：0"
  )
  app$click("app-counter-increment")
  app$click(selector = 'nav a[href$="#!/report"]')
  app$wait_for_js(
    "location.hash === '#!/report' && getComputedStyle(document.querySelector('#app-dice-roll')).visibility === 'hidden'"
  )
  app$run_js("history.back()")
  app$wait_for_js(
    "getComputedStyle(document.querySelector('#app-dice-roll')).visibility === 'visible'"
  )
  testthat::expect_equal(
    app$get_value(output = "app-counter-count"),
    "カウント：1"
  )
  app$run_js("history.forward()")
  app$wait_for_js(
    "document.querySelector('nav a[aria-current=page]')?.textContent === 'レポート'"
  )
  app$get_chromote_session()$Page$reload()
  app$wait_for_js(
    "document.querySelector('nav a[aria-current=page]')?.textContent === 'レポート'"
  )
  testthat::expect_false(app$get_js(
    "getComputedStyle(document.querySelector('#app-dice-roll')).visibility === 'visible'"
  ))
})

testthat::test_that("the counter works while a daemon runs the dice simulation", {
  testthat::skip_if_not(
    Sys.getenv("SHINYTAURI_HEAVY_TESTS") == "1",
    "Enable heavy tests with SHINYTAURI_HEAVY_TESTS=1"
  )
  app <- start_browser()
  on.exit(app$stop(), add = TRUE)

  app$set_inputs(`app-dice-trials` = "100000")
  app$click(selector = "#app-dice-roll")
  app$wait_for_js("document.querySelector('#app-dice-roll').disabled")
  app$click(selector = "#app-counter-increment")
  app$wait_for_js(
    "document.querySelector('#app-counter-count').textContent === 'カウント：1' &&
     document.querySelector('#app-dice-roll').disabled"
  )
  app$wait_for_js(
    "!document.querySelector('#app-dice-roll').disabled",
    timeout = 30000
  )
  testthat::expect_match(
    app$get_value(output = "app-dice-summary"),
    "^100,000 回"
  )
})

check_distribution_filter <- function(app) {
  table <- "window.jQuery('.datatables').data('datatable')"
  app$wait_for_js(paste0("window.jQuery && ", table, "?.rows().count() === 11"))
  testthat::expect_equal(
    app$get_js(paste0(
      table,
      ".column(1).data().toArray().reduce((a,b) => a+b, 0)"
    )),
    1000
  )
  app$run_js(
    "
    const sum = Array.from(document.querySelectorAll('.dtsp-name'))
      .find(el => el.textContent === '2');
    sum.closest('td').click();
  "
  )
  app$wait_for_js(paste0(table, ".rows({search:'applied'}).count() === 1"))
  testthat::expect_equal(
    app$get_js(
      paste0(table, ".rows({search:'applied'}).data()[0][0]")
    ),
    2
  )
  testthat::expect_true(app$get_js(
    "
    Array.from(document.querySelectorAll('.dtsp-nameCont'))
      .some(el => el.lastElementChild.textContent === '1 / 2')
  "
  ))
  app$run_js("document.querySelector('.dtsp-clearAll').click()")
  app$wait_for_js(paste0(table, ".rows({search:'applied'}).count() === 11"))
}

testthat::test_that("a background dashboard downloads the requested snapshot", {
  app <- start_browser()
  on.exit(app$stop(), add = TRUE)
  testthat::expect_false(app$get_js(
    "document.querySelector('#app-report-download').offsetWidth > 0"
  ))
  app$click("app-counter-increment")
  app$click("app-dice-roll")
  app$set_inputs(`app-dice-trials` = "100000")
  app$click(selector = 'nav a[href$="#!/report"]')
  app$wait_for_js(
    "document.querySelector('#app-report-render').offsetWidth > 0",
    timeout = 30000
  )
  check_distribution_filter(app)
  app$click("app-report-render", wait_ = FALSE)
  app$wait_for_js("document.querySelector('#app-report-render').disabled")
  app$click(selector = 'nav a[href$="#!/"]')
  app$click("app-counter-increment", wait_ = FALSE)
  app$wait_for_js(
    "document.querySelector('#app-counter-count').textContent === 'カウント：2'"
  )
  app$click(selector = 'nav a[href$="#!/report"]')
  app$wait_for_js(
    "document.querySelector('#app-report-download').offsetWidth > 0 && document.querySelector('#app-report-download').getAttribute('href')",
    timeout = 180000
  )
  download <- app$get_download("app-report-download")
  doc <- xml2::read_html(download)
  titles <- xml2::xml_text(xml2::xml_find_all(
    doc,
    "//*[contains(@class, 'card-header')]"
  ))
  testthat::expect_true(all(c("カウンター", "試行回数") %in% trimws(titles)))
  values <- xml2::xml_text(xml2::xml_find_all(doc, "//strong"))
  testthat::expect_true(all(c("1", "1,000 回") %in% values))
  testthat::expect_length(
    xml2::xml_find_all(doc, "//link[@rel='stylesheet'] | //script[@src]"),
    0
  )
  styles <- xml2::xml_text(xml2::xml_find_all(doc, "//style"))
  testthat::expect_false(any(grepl(
    "@import\\s+(?:url\\(\\s*)?['\"]?data:",
    styles,
    perl = TRUE
  )))
  browser <- app$get_chromote_session()
  browser$Network$emulateNetworkConditions(
    offline = TRUE,
    latency = 0,
    downloadThroughput = 0,
    uploadThroughput = 0
  )
  path <- normalizePath(download, winslash = "/")
  page <- browser$Page$navigate(paste0(
    "file:///",
    utils::URLencode(sub("^/", "", path))
  ))
  if (identical(page$errorText, "net::ERR_BLOCKED_BY_ADMINISTRATOR")) {
    # Managed Chromium may disallow file navigation. Load the downloaded HTML
    # in a blank test page while the browser network is still offline.
    blank <- browser$Page$navigate("about:blank")
    browser$Page$setDocumentContent(
      frameId = blank$frameId,
      html = paste(readLines(path, encoding = "UTF-8"), collapse = "\n")
    )
  }
  check_distribution_filter(app)
})
