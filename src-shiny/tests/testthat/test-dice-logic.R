box::use(.. / .. / app / logic / dice)

testthat::test_that("rolls produce a validated distribution with the requested total", {
  result <- dice$roll(1000L)
  testthat::expect_true(S7::S7_inherits(result, dice$DiceDistribution))
  testthat::expect_identical(result@total, 1000L)
  testthat::expect_equal(sum(result@observed_percent), 100)
  testthat::expect_equal(sum(dice$expected_percent), 100)
  testthat::expect_error(dice$roll(-1L))
  testthat::expect_error(dice$DiceDistribution(counts = integer(11L)))
})
