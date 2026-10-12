[Variables]
WidgetPadding=#PaddingBase#
WidgetRadius=#RadiusBase#

[AutoThemeMeasure]
Measure=Calc
Formula=#DarkMode#
IfCondition=(#DarkMode# = 1)
IfTrueAction=[!SetVariable BackgroundColor #DarkThemeBackground#][!SetVariable ForegroundColor #DarkThemeForeground#]
IfFalseAction=[!SetVariable BackgroundColor #LightThemeBackground#][!SetVariable ForegroundColor #LightThemeForeground#]

