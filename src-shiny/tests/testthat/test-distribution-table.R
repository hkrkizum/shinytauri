box::use(
  .. / .. / app / logic / dice[DiceDistribution],
  .. / .. / app / view / distribution[distribution_table]
)

testthat::test_that("the filterable table represents the completed simulation", {
  counts <- 1:11
  table <- distribution_table(DiceDistribution(counts = counts))
  testthat::expect_s3_class(table, "datatables")
  testthat::expect_identical(table$x$data[[1L]], 2:12)
  testthat::expect_identical(table$x$data[[2L]], counts)
  testthat::expect_true(table$x$options$searchPanes$viewTotal)
  testthat::expect_error(DiceDistribution(counts = integer(11L)))
})
