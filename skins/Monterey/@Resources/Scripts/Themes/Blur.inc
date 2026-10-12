[Variables]
WidgetPadding=0
WidgetRadius=#RadiusBase#

[FrostedGlass]
Measure=Plugin
Plugin=FrostedGlass
Type=Acrylic
Corner=Round

[ChameleonMeasure]
Measure=Plugin
Group=ColorTheme
Plugin=Chameleon
Type=Desktop

[DarkMode]
Measure=Plugin
Group=ColorTheme
Plugin=Chameleon
Parent=ChameleonMeasure
Color=Luminance
RegExpSubstitute=1
Substitute="0.[4-9].*":"0","0.[0-3].*":"1"
OnUpdateAction=[!SetVariable DarkMode [DarkMode]]

[BlurThemeBackground]
Measure=String
String=#DarkMode#
Substitute="0":"ffffff","1":"333333"
OnUpdateAction=[!SetVariable BackgroundColor [&#CURRENTSECTION#]]
DynamicVariables=1

[BlurThemeForeground]
Measure=String
String=#DarkMode#
Substitute="0":"000000","1":"ffffff"
OnUpdateAction=[!SetVariable ForegroundColor [&#CURRENTSECTION#]]
DynamicVariables=1

[BackgroundMeter]
Shape=Rectangle 0, 0, #WidgetWidth#, #WidgetHeight#, #WidgetRadius#, #WidgetRadius# | Fill Color #BackgroundColor#90 | StrokeWidth 0
