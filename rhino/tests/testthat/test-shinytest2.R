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
