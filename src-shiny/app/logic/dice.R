#' Supported numbers of rolls for one simulation.
#' @export
trial_sizes <- c(1000L, 10000L, 100000L)

#' Possible sums of two six-sided dice, in count-vector order.
#' @export
sums <- 2:12

#' Theoretical percentages of the possible sums.
#' @export
expected_percent <- 100 * (6 - abs(sums - 7)) / 36

#' Distribution of the sums of two dice
#' @param counts Integer frequencies for sums two through twelve.
#' @return A validated distribution with total and observed percentages.
#' @export
DiceDistribution <- S7::new_class(
  "DiceDistribution",
  properties = list(
    counts = S7::new_property(
      S7::class_integer,
      validator = function(value) {
        if (
          length(value) != length(sums) ||
            anyNA(value) ||
            any(value < 0L) ||
            !any(value > 0L)
        ) {
          "must contain eleven non-negative counts with a positive total"
        }
      }
    ),
    total = S7::new_property(S7::class_integer, getter = function(self) {
      sum(self@counts)
    }),
    observed_percent = S7::new_property(
      S7::class_numeric,
      getter = function(self) 100 * self@counts / self@total
    )
  )
)

#' Simulate two independent six-sided dice
#' @param n A supported positive integer number of trials.
#' @return A DiceDistribution containing the observed frequencies.
#' @export
roll <- S7::new_generic("roll", "n")
S7::method(roll, S7::class_integer) <- function(n) {
  if (length(n) != 1L || !n %in% trial_sizes) {
    stop("Unsupported trial count")
  }
  values <- sample.int(6L, n, replace = TRUE) +
    sample.int(6L, n, replace = TRUE)
  DiceDistribution(counts = tabulate(values - 1L, nbins = length(sums)))
}
