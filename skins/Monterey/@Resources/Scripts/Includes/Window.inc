[Rainmeter]
Update=1000
DefaultUpdateDivider=-1
DefaultSnapEdges=0
DefaultKeepOnScreen=0
DragMargins=0,0,0,-45
Group=Monterey
RightMouseUpAction=[]

[ThemeAsNumber]
Measure=String
String=#Theme#
DynamicVariables=1
Substitute="Light":"1","Dark":"2","Auto":"3","Color":"4","Blur":"5"

[BackgroundMeter]
Meter=Shape
DynamicVariables=1
Shape=Rectangle 0, 0, #WidgetWidth#, #WidgetHeight#, #WidgetRadius#, #WidgetRadius# | Fill Color #BackgroundColor# | StrokeWidth 0.5 | Stroke Color 808080

[Variables]
WidgetRadius = #RadiusBase#
@Include1=#@#Variables\Global.inc
@Include2=#@#Scripts\Includes\Context.inc
@Include3=#@#Scripts\Themes\#Theme#.inc

WidgetCenterX = (#WidgetPadding# + #WidgetWidth# / 2)
WidgetCenterY = (#WidgetPadding# + #WidgetHeight# / 2)