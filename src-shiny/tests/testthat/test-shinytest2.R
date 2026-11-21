start_browser <- function() {
  previous <- options(chromote.timeout = 30)
  on.exit(options(previous), add = TRUE)
  # AppDriver skips when Chromium cannot start. Browser setup errors must fail CI.
  tryCatch(
    shinytest2::AppDriver$new(),
    skip = function(e) stop(conditionMessage(e), call. = FALSE)
  )
}

testthat::test_that("the counter responds in the desktop UI", {
  app <- start_browser()
  on.exit(app$stop(), add = TRUE)

  testthat::expect_equal(app$get_value(output = "app-count"), "カウント：0")
  app$click("app-increment")
  testthat::expect_equal(app$get_value(output = "app-count"), "カウント：1")
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
  app$click(selector = "#app-increment")
  app$wait_for_js(
    "document.querySelector('#app-count').textContent === 'カウント：1' &&
     document.querySelector('#app-dice-roll').disabled"
  )
  app$wait_for_js("!document.querySelector('#app-dice-roll').disabled")
  testthat::expect_match(
    app$get_value(output = "app-dice-summary"),
    "^100,000 回"
  )
})

testthat::test_that("a background dashboard downloads the requested snapshot", {
  app <- start_browser()
  on.exit(app$stop(), add = TRUE)
  testthat::expect_false(app$get_js(
    "document.querySelector('#app-report-download').offsetWidth > 0"
  ))
  app$click("app-increment")
  app$click("app-dice-roll")
  app$wait_for_js(
    "document.querySelector('#app-report-render').offsetWidth > 0"
  )
  app$set_inputs(`app-dice-trials` = "100000")
  app$click("app-report-render", wait_ = FALSE)
  app$wait_for_js("document.querySelector('#app-report-render').disabled")
  app$click("app-increment", wait_ = FALSE)
  app$wait_for_js(
    "document.querySelector('#app-count').textContent === 'カウント：2'"
  )
  app$wait_for_js(
    "document.querySelector('#app-report-download').offsetWidth > 0 && document.querySelector('#app-report-download').getAttribute('href')",
    timeout = 180000
  )
  doc <- xml2::read_html(app$get_download("app-report-download"))
  titles <- xml2::xml_text(xml2::xml_find_all(
    doc,
    "//*[contains(@class, 'card-header')]"
  ))
  testthat::expect_true(all(c("カウンター", "試行回数") %in% trimws(titles)))
  values <- xml2::xml_text(xml2::xml_find_all(doc, "//strong"))
  testthat::expect_true(all(c("1", "1,000 回") %in% values))
  observations <- xml2::xml_text(xml2::xml_find_all(doc, "//tbody/tr/td[2]"))
  testthat::expect_length(observations, 11)
  testthat::expect_equal(sum(as.integer(observations)), 1000)
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
})
