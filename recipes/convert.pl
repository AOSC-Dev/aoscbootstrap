#!/bin/perl
use v5.19;
use strict;
use File::Temp qw(tempfile);
use Data::Dumper;

sub strip_actionables ($) {
    my $str = shift;
    my $stripped = $str =~ s/#\s+Common functions\..+//gsr;
    return $stripped;
}

sub recipe_names ($) {
    my $str = shift;
    $str =~ m/# Specific recipes.+/msp;
    my $match = ${^MATCH};
    my @names = ();
    while ($match =~ /^([A-Z]+_RECIPE)=/gm) {
        push @names, $1;
    }
    return @names;
}

sub generate_output_script ($$) {
    my $script = shift;
    my $path = shift;
    my @names = recipe_names($script);
    my $stub_script = strip_actionables($script);
    $stub_script .= "set -e\n";
    foreach my $d (@names) {
        my $filename = $d =~ s/_RECIPE//gr;
        $filename = lc $filename;
        $stub_script .= "echo \"\$$d\" | tr ' ' '\\n' > '$path/$filename.lst'\n";
    }
    return $stub_script;
}

my $f = shift;
my $path = shift;
my $str = do { local ( @ARGV, $/ ) = $f; <> };
print STDERR "Converting ciel-generate script to plain recipe...\n";
my $script = generate_output_script($str, $path);
my ( $fh, $filename ) = tempfile( "ab-XXXXXX", SUFFIX => '.sh', DIR => '/tmp/' )
      or die("Cannot create temporary file");
print $fh "$script\n";
close $fh;
system("bash", '-e', "$filename") == 0 || die "Error";
