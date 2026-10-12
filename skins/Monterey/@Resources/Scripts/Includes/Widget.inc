[Rainmeter]
DefaultUpdateDivider=-1
RightMouseUpAction=[!SetVariable Context "Widget"][!SkinCustomMenu]
Group=Monterey

[ThemeAsNumber]
Measure=String
String=#Theme#
DynamicVariables=1
Substitute="Light":"1","Dark":"2","Auto":"3","Color":"4","Blur":"5"

[BackgroundMeter]
Meter=Shape
Shape=Rectangle 0, 0, #WidgetWidth#, #WidgetHeight#, #WidgetRadius#, #WidgetRadius# | Fill Color #BackgroundColor# | StrokeWidth 0
Padding=#WidgetPadding#, #WidgetPadding#, #WidgetPadding#, #WidgetPadding#
DynamicVariables=1

[Variables]
@Include=#@#Variables\Global.inc

WidgetPadding=#PaddingBase#
WidgetRadius=#RadiusBase#
WidgetCenterX=(#WidgetPadding# + #WidgetWidth# / 2)
WidgetCenterY=(#WidgetPadding# + #WidgetHeight# / 2)

@Include2=#@#Languages\#Language#\Widget.inc
@Include3=#@#Scripts\Themes\#Theme#.inc
@Include4=#@#Scripts\Sizes\#WidgetSize#.inc
@Include5=#@#Scripts\Includes\Context.inc
@Include6=#@#Scripts\Contexts\Widget.inc
