{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };
  outputs = flakes:
    let
      system = "x86_64-linux";
      pkgs = import flakes.nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      llvm = pkgs.llvmPackages_17;
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.bashInteractive
          pkgs.rustup
        ];
        LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ llvm.libclang.lib ];
      };
    };
}

