// LED lure bezel for console — blue-visible ring mount
$fn = 80;
outer = 90;
inner = 70;
h = 8;
difference() {
    cylinder(h=h, d=outer);
    translate([0,0,-1]) cylinder(h=h+2, d=inner);
}
// cable notch
translate([outer/2-6, -3, 0]) cube([8, 6, h]);
