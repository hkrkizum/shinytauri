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

testthat::test_that("column labels survive loading the module in a C locale", {
  labels <- callr::r(
    function() {
      Sys.setlocale("LC_CTYPE", "C")
      options(box.path = getwd())
      box::use(
        app / logic / dice[DiceDistribution],
        app / view / distribution[distribution_table]
      )
      table <- distribution_table(DiceDistribution(counts = 1:11))
      enc2utf8(names(table$x$data))
    },
    wd = testthat::test_path("..", "..")
  )
  testthat::expect_identical(
    labels,
    c("合計", "観測回数", "観測割合 (%)", "理論割合 (%)")
  )
})
