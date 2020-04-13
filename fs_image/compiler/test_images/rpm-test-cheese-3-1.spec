Name:   rpm-test-cheese
Version:  3
Release:  1
Summary:  The "cheese" package.

Group:    Facebook/script
BuildArch: noarch
License:  MIT

BuildRequires:  coreutils

%prep

%description

%build

%install
mkdir -p %{buildroot}/rpm_test/
cat >%{buildroot}/rpm_test/cheese3.txt <<EOF
This is the third goat cheese text.
EOF

%clean

%files
/*
