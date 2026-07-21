---
title: Statistics
description: Compare groups, test correlations, check normality, and run ANOVA from a data table.
---

PlotX runs classical statistics directly on a data table. Each **column** is a
sample, and each **row** lines the columns up, so a paired comparison uses the
two values on the same row. You start from the question you want to answer, not
the name of a test.

## Opening the tools

Select a data table, open the **Analyze** tab, and choose **Statistics**. A
Statistics task card opens at the upper-right of the canvas. The card walks you
through three steps:

1. **What do you want to find out?** Pick the plain-language question.
2. **Data roles.** Choose which columns take part and what each one means.
3. **Options.** Set the direction, the variance assumption, or the confidence
   level where they apply.

The card names the formal test it will run (for example, *Runs the Welch's t
test*), so an experienced user can confirm the method. Results are saved with
the table and stay available after you close the card.

## Prepare your table

- Put each group or measurement in its own **column**. Give columns clear names;
  those names appear throughout the results.
- For paired data and correlation, put the two measurements for the same subject
  on the **same row**.
- For a two-way analysis, use the **long layout**: one column of values, one
  column that codes the first factor, and one column that codes the second
  factor. Factor columns hold numeric codes (for example `0` and `1`, or `1`,
  `2`, `3`); PlotX shows you the levels it detected so you can confirm they are
  categories and not a continuous measurement.

Blank or non-finite cells are never dropped silently. When a choice would
exclude cells or rows, the card says how many and asks you to tick **Exclude the
affected cells or rows and continue** before it will run. The sample sizes used
are always reported with the result.

## Compare two independent groups

Use this when two columns hold measurements from two separate sets of subjects.

1. Question: **Compare two independent groups**.
2. Choose **Group A** and **Group B**.
3. Choose the variance assumption:
   - **Welch** does not assume the two groups have equal spread. Use it if you
     are unsure.
   - **Student** assumes both groups share the same spread and pools it.
4. Choose the **direction**. Two-sided looks for a difference either way. The
   one-sided options are spelled out with your column names, such as *Control
   less than Treated*.

The result reports the difference **A − B**, its confidence interval, the t
statistic and degrees of freedom, the p-value, and Cohen's d, along with the
number of values used in each group.

## Compare paired or before/after measurements

Use this when each row pairs two measurements of the same subject, such as a
value before and after a treatment.

1. Question: **Compare paired or before/after measurements**.
2. Choose **Column A** and **Column B**. The test uses the per-row difference
   **A − B**, so the direction is stated relative to that subtraction.

Only rows where both columns have a value are used; rows with a blank in either
column are excluded once you confirm.

## Compare one column with a reference value

Use this to test whether a column's mean differs from a known target.

1. Question: **Compare one column with a reference value**.
2. Choose the column and enter the **reference value**.
3. Choose the direction relative to the reference.

## Compare three or more groups

Use one-way ANOVA to compare the means of several columns at once.

1. Question: **Compare three or more groups**.
2. Tick every group column you want to compare.
3. Leave **Also compare each pair of groups (Tukey HSD)** on to get the pairwise
   comparisons.

The result lists each group's mean and size, the ANOVA table (F, degrees of
freedom, p-value) and the η² and ω² effect sizes. When Tukey HSD is on, each
pair of groups gets a difference, a simultaneous confidence interval, and a
family-wise p-value. The pairwise comparisons run regardless of the overall
p-value — a non-significant omnibus test does not hide them.

## Study two factors at once

Use two-way ANOVA to look at two grouping factors together, and their
interaction.

1. Question: **Study two factors at once**.
2. Choose the **Value column** and the two **Factor** columns.
3. Check the detected levels shown under each factor. If a factor lists many
   values, it may be a continuous measurement rather than a category, and the
   card warns you.

With more than one observation per factor combination, the interaction is
tested against the within-cell spread. With exactly one observation per
combination, the interaction cannot be separated from error, and the result
says so instead of reporting an interaction. Every combination of levels must
appear at least once.

## Check whether a column looks normal

Use the Shapiro–Wilk test to see how well a column agrees with a normal
distribution, for 3 to 5000 values.

1. Question: **Check whether a column looks normal**.
2. Tick the columns to check.

A high p-value means the sample is **consistent with** a normal distribution; it
does not prove the data are normal. A low p-value indicates a departure from
normality.

## See whether two columns are related

1. Question: **See whether two columns are related**.
2. Choose the two columns and the method:
   - **Pearson** measures a straight-line relationship.
   - **Spearman** ranks the values first, capturing any consistent rise or fall
     and resisting outliers.

Only rows where both columns have a value are used. The result reports the
correlation coefficient, the test statistic and degrees of freedom, and the
two-sided p-value.

## Summarize columns

Question **Summarize one or more columns** reports, for each ticked column, the
count, mean and median, standard deviation and standard error, the minimum,
quartiles and maximum, the interquartile range, and skewness and excess
kurtosis. Some measures need a minimum sample size and are shown as *n/a* until
it is reached.

## Read, keep, and reuse results

Every result is saved with the table under **Saved results** and answers your
question first. Expand **Details and full numbers** for the complete, checkable
values: estimates, statistics, degrees of freedom, p-values, confidence
intervals, effect sizes, and the sample sizes used.

- **Copy** puts the full labelled result on the clipboard as text you can paste
  into notes or a spreadsheet.
- **Add table to board** turns a summary, normality, or group-means result into
  a new data table you can plot or export like any other. Single-number results
  (one t test, one correlation) are best copied as text.

Results are part of the project, so they are still there when you reopen it.

## Reading the numbers responsibly

- A p-value above your threshold does **not** prove there is no difference; it
  means the data do not show one at that threshold. Report the estimate and its
  confidence interval, not just "significant" or "not significant".
- Differences and effect sizes always carry a direction. Check which way the
  subtraction goes (**A − B**) before interpreting the sign.
- State what your uncertainty measures mean (SD, SEM, or another) in your figure
  caption; PlotX shows the numbers as computed.
