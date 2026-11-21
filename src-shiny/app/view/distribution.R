box::use(.. / logic / dice, .. / types[typed])

#' Create the SearchPanes table used in Shiny and the offline report
#' @param distribution A completed DiceDistribution.
#' @return A DT datatables htmlwidget.
#' @export
distribution_table <- S7::new_generic("distribution_table", "distribution")
S7::method(distribution_table, dice$DiceDistribution) <- function(
  distribution
) {
  data <- data.frame(
    sum = dice$sums,
    count = distribution@counts,
    observed = round(distribution@observed_percent, 1),
    expected = round(dice$expected_percent, 1)
  )
  names(data) <- c("合計", "観測回数", "観測割合 (%)", "理論割合 (%)")
  pane_columns <- c(0L, 3L)
  DT::datatable(
    data,
    rownames = FALSE,
    selection = "none",
    extensions = c("Select", "SearchPanes"),
    options = list(
      dom = "Pti",
      paging = FALSE,
      searchPanes = list(
        columns = pane_columns,
        layout = "columns-2",
        viewTotal = TRUE
      ),
      columnDefs = list(list(
        targets = pane_columns,
        searchPanes = list(show = TRUE)
      )),
      language = list(
        info = "_TOTAL_ 件",
        infoEmpty = "0 件",
        infoFiltered = "（全 _MAX_ 件）",
        zeroRecords = "該当する出目はありません",
        searchPanes = list(
          title = "絞り込み（選択中: %d）",
          clearMessage = "すべて解除",
          collapseMessage = "すべて閉じる",
          showMessage = "すべて開く",
          countFiltered = "{shown} / {total}"
        )
      )
    )
  ) |>
    typed(S7::new_S3_class("datatables"))
}
