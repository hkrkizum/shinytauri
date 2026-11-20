box::use(.. / .. / app / view / dice)

testthat::test_that("unsupported trial counts do not start a dice simulation", {
  shiny::testServer(dice$server, {
    session$setInputs(roll = 0L)
    session$setInputs(trials = "-1", roll = 1L)
    testthat::expect_identical(task$status(), "initial")
    testthat::expect_match(output$summary, "試行回数")
  })
})
